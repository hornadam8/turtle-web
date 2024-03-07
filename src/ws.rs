use leptos::logging;
use std::cell::RefCell;
use std::collections::HashMap;
use web_sys::{ErrorEvent, MessageEvent, WebSocket};

use wasm_bindgen::prelude::*;

use std::error::Error;
use std::fmt::Formatter;
use std::str::{from_utf8, FromStr};

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use turtle_protocol::{
    IntoReceivable, IntoSendable, LoginFail, LoginMessage, LoginSuccess, WsShell,
};

thread_local! {
    static RUNTIME: Runtime = Runtime::new();
}

pub fn connect(addr: String, username: String, password: String) {
    RUNTIME.with(|r| {
        r.set_addr(addr);
        r.set_username(username);
        r.set_password(password);
        r.connect();
    });
}

pub fn set_open_hook(f: impl FnMut() + 'static) {
    RUNTIME.with(|r| {
        r.set_open_hook(f);
    });
}
pub fn set_error_hook(f: impl FnMut(ErrorEvent) + 'static) {
    RUNTIME.with(|r| {
        r.set_error_hook(f);
    });
}
pub fn set_close_hook(f: impl FnMut() + 'static) {
    RUNTIME.with(|r| {
        r.set_close_hook(f);
    });
}

pub fn register_handler<T>(f: impl IntoReceivable<T>) {
    RUNTIME.with(|r| {
        let (msg_type, f) = f.into_receivable();
        r.register_handler(msg_type, f);
    });
}

pub fn send_message(msg: impl IntoSendable) {
    let ws_msg = msg.into_sendable();
    RUNTIME.with(move |r| {
        r.send_message(ws_msg);
    })
}

type JsOpenHandler = Option<Closure<dyn FnMut()>>;
type JsMessageHandler = Option<Closure<dyn FnMut(MessageEvent)>>;
type JsErrorHandler = Option<Closure<dyn FnMut(ErrorEvent)>>;
type JsCloseHandler = Option<Closure<dyn FnMut()>>;
type MessageHandlerRegistry = HashMap<String, Vec<Box<dyn FnMut(WsShell)>>>;

#[derive(Default)]
pub struct Runtime {
    username: RefCell<Option<String>>,
    password: RefCell<Option<String>>,
    addr: RefCell<Option<String>>,
    ws: RefCell<Option<WebSocket>>,
    onopen: RefCell<JsOpenHandler>,
    onmessage: RefCell<JsMessageHandler>,
    onerror: RefCell<JsErrorHandler>,
    onclose: RefCell<JsCloseHandler>,
    reconnector: RefCell<Option<Closure<dyn Fn()>>>,
    msg_handlers: RefCell<MessageHandlerRegistry>,
    open_hook: RefCell<Option<Box<dyn FnMut()>>>,
    error_hook: RefCell<Option<Box<dyn FnMut(ErrorEvent)>>>,
    close_hook: RefCell<Option<Box<dyn FnMut()>>>,
}

impl Runtime {
    fn new() -> Self {
        // make the handlers
        let onopen = RefCell::new(Some(Closure::<dyn FnMut()>::new(move || {
            logging::log!("Runtime opened websocket");
            Runtime::run_open_hook();
            Runtime::try_login();
        })));

        let onmessage = RefCell::new(Some(Closure::<dyn FnMut(_)>::new(
            move |e: MessageEvent| {
                //logging::log!("Runtime got a message");
                if let Some(s) = e.data().as_string() {
                    logging::log!("ws message: {s}");
                    let ws_msg: Result<WsShell, _> = serde_json::from_str(&s);
                    match ws_msg {
                        Ok(ws_msg) => Runtime::handle_ws_msg(ws_msg),
                        _ => logging::error!("invalid ws message!"),
                    }
                } else if let Ok(blob) = e.data().dyn_into::<web_sys::Blob>() {
                    logging::log!("Got a blog! {:?}", blob);
                } else {
                    logging::error!("ws message failed to get string!");
                    logging::log!("e.data() = {:?}", e.data());
                }
            },
        )));

        let onerror = RefCell::new(Some(Closure::<dyn FnMut(_)>::new(move |e: ErrorEvent| {
            logging::error!("ws error: {e:?}");
            Runtime::run_error_hook(e);
        })));

        let reconnector = RefCell::new(Some(Closure::<dyn Fn()>::new(|| {
            logging::log!("Reconnecting...");
            Runtime::reconnect();
        })));

        let onclose = RefCell::new(Some(Closure::<dyn FnMut()>::new(move || {
            logging::log!("closed connection!\nReconnecting in 1 second...");
            Runtime::run_close_hook();
            Runtime::set_reconnect_timeout();
        })));

        Self {
            onopen,
            onmessage,
            onerror,
            onclose,
            reconnector,
            ..Default::default()
        }
    }

    fn set_username(&self, username: String) {
        let mut username_slot = self.username.borrow_mut();
        *username_slot = Some(username);
    }

    fn set_password(&self, password: String) {
        let mut password_slot = self.password.borrow_mut();
        *password_slot = Some(password);
    }

    fn set_addr(&self, addr: String) {
        let mut addr_slot = self.addr.borrow_mut();
        *addr_slot = Some(addr);
    }

    fn connect(&self) {
        let maybe_addr = self.addr.borrow();
        if maybe_addr.is_none() {
            logging::error!("Runtime error: calling connect when no address is set");
            return;
        }
        let addr = maybe_addr.as_ref().unwrap();
        logging::log!("Connecting to {addr}");

        let onopen = self.onopen.borrow();
        let onmessage = self.onmessage.borrow();
        let onerror = self.onerror.borrow();
        let onclose = self.onclose.borrow();

        // create the websocket
        let ws = WebSocket::new(addr).expect("can construct a WebSocket");

        // attach the handlers
        match (
            onopen.as_ref(),
            onmessage.as_ref(),
            onerror.as_ref(),
            onclose.as_ref(),
        ) {
            (Some(onopen), Some(onmessage), Some(onerror), Some(onclose)) => {
                ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
                ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
                ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            }
            _ => {
                logging::error!("uh oh! can't set the handlers?!");
            }
        };
        // save the ws
        let mut ws_cell = self.ws.borrow_mut();
        *ws_cell = Some(ws);
    }

    fn register_handler(&self, msg_type: String, f: impl FnMut(WsShell) + 'static) {
        let mut msg_handlers = self.msg_handlers.borrow_mut();
        let entry = msg_handlers.entry(msg_type).or_default();
        entry.push(Box::new(f));
    }

    fn set_open_hook(&self, f: impl FnMut() + 'static) {
        let mut slot = self.open_hook.borrow_mut();
        *slot = Some(Box::new(f));
    }

    fn set_error_hook(&self, f: impl FnMut(ErrorEvent) + 'static) {
        let mut slot = self.error_hook.borrow_mut();
        *slot = Some(Box::new(f));
    }

    fn set_close_hook(&self, f: impl FnMut() + 'static) {
        let mut slot = self.close_hook.borrow_mut();
        *slot = Some(Box::new(f));
    }

    fn handle_msg(&self, msg: WsShell) {
        let mut handlers = self.msg_handlers.borrow_mut();
        let t = &msg.type_;
        let maybe_fs = handlers.get_mut(t);
        match maybe_fs {
            Some(fs) => {
                logging::log!("handling msg of type {t} with {} handlers", fs.len());
                for f in fs {
                    f(msg.clone())
                }
            }
            None => {
                logging::error!("No handlers registered for {t} messages");
            }
        }
    }

    fn send_message(&self, ws_msg: WsShell) {
        let maybe_json = serde_json::to_string(&ws_msg);
        let maybe_ws = self.ws.borrow();
        if let (Ok(json), Some(ws)) = (maybe_json, maybe_ws.as_ref()) {
            let res = ws.send_with_str(&json);
            if res.is_err() {
                logging::error!("uh oh! Failed to send JSON!");
            }
        }
    }

    fn reconnect() {
        RUNTIME.with(|r| {
            r.connect();
        })
    }

    fn try_login() {
        RUNTIME.with(|r| {
            let maybe_username = r.username.borrow();
            let maybe_password = r.password.borrow();
            if let (Some(username), Some(password)) =
                (maybe_username.as_ref(), maybe_password.as_ref())
            {
                send_message(LoginMessage {
                    username: username.clone(),
                    password: password.clone(),
                });
            }
        });
    }

    fn run_open_hook() {
        RUNTIME.with(|r| {
            let mut maybe_open_hook = r.open_hook.borrow_mut();
            if let Some(mut box_f) = maybe_open_hook.take() {
                box_f();
                *maybe_open_hook = Some(box_f);
            }
        });
    }

    fn run_error_hook(e: ErrorEvent) {
        RUNTIME.with(|r| {
            let mut maybe_error_hook = r.error_hook.borrow_mut();
            if let Some(mut box_f) = maybe_error_hook.take() {
                box_f(e);
                *maybe_error_hook = Some(box_f);
            }
        });
    }

    fn run_close_hook() {
        RUNTIME.with(|r| {
            let mut maybe_close_hook = r.close_hook.borrow_mut();
            if let Some(mut box_f) = maybe_close_hook.take() {
                box_f();
                *maybe_close_hook = Some(box_f);
            }
        })
    }

    fn handle_ws_msg(msg: WsShell) {
        RUNTIME.with(|r| {
            r.handle_msg(msg);
        });
    }

    fn set_reconnect_timeout() {
        RUNTIME.with(|r| {
            let reconnector = r.reconnector.borrow();
            set_timeout(reconnector.as_ref().unwrap(), 1000);
        });
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(f: &Closure<dyn Fn()>, ms: u32) -> u32;
}
