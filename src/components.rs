use crate::{
    mailroom::Mailroom,
    ws::{connect, register_handler, send_message, set_close_hook, set_error_hook, set_open_hook},
};
use leptos::html::{Div, Input};
use leptos::*;
use std::string::ToString;
use turtle_protocol::{
    ChannelAdded, ChannelId, ChannelsInfo, ChatMessage, CreateChannel, LoginFail, LoginSuccess,
    SendChatMessage, UserId, UserJoined, UserLeft, UsersInfo,
};
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = localStorage)]
    type LocalStorage;

    #[wasm_bindgen(static_method_of = LocalStorage, js_class = "localStorage", js_name = getItem)]
    fn get_item(key: String) -> Option<String>;

    #[wasm_bindgen(static_method_of = LocalStorage, js_class = "localStorage", js_name = setItem)]
    fn set_item(key: String, value: String);

    #[wasm_bindgen(static_method_of = LocalStorage, js_class = "localStorage", js_name = removeItem)]
    fn remove_item(key: String);

    #[wasm_bindgen(js_name = location)]
    type Location;

    #[wasm_bindgen(static_method_of = Location, js_class="location")]
    fn reload() /* -> ! */; // never type is still experimental, also doesn't impl WASM abi

    type Date;

    #[wasm_bindgen(constructor)]
    fn new(ts: f64) -> Date; // todo: make ts a u64? that makes it a BigInt in JS land and makes this conversion trickier

    #[wasm_bindgen(method, js_name = toLocaleString)]
    fn to_locale_string(this: &Date) -> String;

    #[wasm_bindgen(js_name = getWsAddress)]
    fn get_ws_address() -> String;

}

#[component]
pub fn App() -> impl IntoView {
    // create the mailroom
    let (mailroom, set_mailroom) = create_signal(Mailroom::new(ChannelId(1)));
    // and install it into the floorboard
    provide_context(mailroom);
    provide_context(set_mailroom);

    // display main or login?
    let (display_main_view, set_display_main_view) = create_signal(false);

    create_effect(move |_| {
        set_open_hook(|| {
            logging::log!("app knows we openned the websocket!");
        });

        set_error_hook(|e| {
            logging::log!("app knows we got an error: {}", e.message());
        });

        set_close_hook(|| {
            logging::log!("app knows the ws is closed");
        });

        register_handler(move |success: LoginSuccess| {
            logging::log!("Login result: {success:?}");
            let mailroom = mailroom.get_untracked();
            mailroom.set_current_user_id(success.id);
            set_mailroom(mailroom);
            if !display_main_view.get_untracked() {
                set_display_main_view(true);
            }
        });

        register_handler(move |fail: LoginFail| {
            logging::log!("Failed to login! Reason: {}", fail.reason);
        });
    });

    // see if we have a saved username and password
    let maybe_username = LocalStorage::get_item("username".to_string());
    let maybe_password = LocalStorage::get_item("password".to_string());
    // and try to login
    create_effect(move |_| {
        if let (Some(u), Some(p)) = (maybe_username.as_ref(), maybe_password.as_ref()) {
            logging::log!("Got the saved username and password! Going to try connecting now");
            connect(get_ws_address(), u.clone(), p.clone());
        }
    });

    let what_to_display = move || {
        if display_main_view() {
            logging::log!("Displaying main view...");
            view! {
                <div class="p-2 h-full flex flex-col">
                    <div class="flex flex-row">
                        <h1 class="text-3xl p-2 grow font-bold text-amber-300">Turtle Chat</h1>
                        <button class="px-2 font-bold text-xl text-rose-500 hover:underline"
                            on:click=move |_| {
                                LocalStorage::remove_item("username".to_string());
                                Location::reload();
                            }
                        >

                            Logout
                        </button>
                    </div>
                    <div class="flex flex-row grow">
                        <Sidebar />
                        <Chat />
                    </div>
                </div>
            }
        } else {
            logging::log!("Displaying login view...");
            view! {
                <div class="w-full max-w-xs mx-auto">
                    <h1 class="text-3xl font-bold py-4 text-amber-300">Login</h1>
                    <Login />
                </div>
            }
        }
    };

    view! {
        <main class="h-screen bg-green-900 w-full">
            {what_to_display}
        </main>
    }
}

#[component]
fn Login() -> impl IntoView {
    let (username, set_username) = create_signal("".to_string());
    let (password, set_password) = create_signal("".to_string());

    view! {
        <form class="rounded shadow-md bg-white px-8 py-8"
            on:submit=move |evt| {
                evt.prevent_default();
                let username = username();
                let password = password();
                if username.len() > 0 {
                    // save em in localStorage
                    LocalStorage::set_item("username".to_string(), username.clone());
                    LocalStorage::set_item("password".to_string(), password.clone());
                    // connect to le server
                    connect(get_ws_address(), username, password);
                }
            }
        >
            <div class="mb-4">
                <label class="block text-gray-700 text-sm font-bold mb-2">
                    Username
                </label>
                <input class="shadow border rounded w-full py-2 px-3"
                    type="text"
                    on:input=move |evt| {
                        set_username(event_target_value(&evt).to_string());
                    }
                />
            </div>
            <div class="mb-4">
                <label class="block text-gray-700 text-sm font-bold mb-2">
                    Password
                </label>
                <input class="shadow border rounded w-full py-2 px-3"
                    type="password"
                    on:input=move |evt| {
                        set_password(event_target_value(&evt).to_string());
                    }
                />
            </div>
            <div class="">
                <button class="bg-amber-500 hover:bg-amber-700 text-white font-bold py-2 px-3 rounded">Login</button>
            </div>
        </form>
    }
}

#[component]
fn Sidebar() -> impl IntoView {
    let mailroom: ReadSignal<Mailroom> = expect_context();
    let set_mailroom: WriteSignal<Mailroom> = expect_context();

    create_effect(move |_| {
        logging::log!("<Sidebar/> effect running");

        register_handler(move |info: ChannelsInfo| {
            let mailroom = mailroom.get_untracked();
            mailroom.add_channels(info);
            set_mailroom(mailroom);
        });

        register_handler(move |channel_added: ChannelAdded| {
            let mailroom = mailroom.get_untracked();
            let cid = channel_added.channel.id;
            mailroom.add_channel(channel_added.channel);
            if mailroom.current_user_id() == Some(channel_added.created_by) {
                mailroom.set_active(cid);
            }
            set_mailroom(mailroom);
        });

        register_handler(move |users_info: UsersInfo| {
            let mailroom = mailroom.get_untracked();
            mailroom.add_users(users_info);
            set_mailroom(mailroom);
        });

        register_handler(move |user_joined: UserJoined| {
            let mailroom = mailroom.get_untracked();
            mailroom.add_user(user_joined.user);
            set_mailroom(mailroom);
        });

        register_handler(move |user_left: UserLeft| {
            let mailroom = mailroom.get_untracked();
            mailroom.remove_user(user_left.id);
            set_mailroom(mailroom);
        });
    });

    let (show_channel_add, set_show_channel_add) = create_signal(false);
    let (new_channel_name, set_new_channel_name) = create_signal("".to_string());
    let new_channel_ref: NodeRef<Input> = create_node_ref();

    let get_channel_list = move || {
        let mailroom = mailroom();
        mailroom.channel_list()
    };

    let get_user_list = move || {
        let mailroom = mailroom();
        mailroom.user_list()
    };

    let add_channel_form = move || {
        if show_channel_add() {
            Some(view! {
                <form class="flex flex-row mx-2 my-2"
                    on:submit=move |evt| {
                        evt.prevent_default();
                        let new_channel_name = new_channel_name();
                        send_message(CreateChannel {
                            name: new_channel_name
                        });
                        set_new_channel_name("".to_string());
                        set_show_channel_add(false);
                    }
                >
                    <input class="p-1 grow rounded text-white bg-emerald-900" type="text"
                        on:input=move |evt| {
                            // trim so no spaces
                            // todo: better channel name filtering? also server-side
                            let channel_name = event_target_value(&evt).trim().to_string();
                            set_new_channel_name(channel_name);
                        }
                        prop:value=new_channel_name
                        node_ref=new_channel_ref
                    />
                    <button class="ml-2 bg-amber-500 hover:bg-emerald-900 text-white p-1 rounded">Add</button>
                </form>
            })
        } else {
            None
        }
    };

    view! {
        <div class="basis-1/4 h-full text-amber-300 bg-green-950 rounded-l-md flex flex-col">
            <div class="h-1/2 flex flex-col">
                <div class="flex flex-row">
                    <h2 class="font-bold text-lg mx-3 my-2 grow">
                        Channels
                    </h2>
                    <button
                        class=move || {
                            if show_channel_add() {
                                "text-sm ml-3 mt-1 mr-3.5 px-2 text-white bg-rose-800"
                            } else {
                                "font-bold text-lg mr-3.5 pt-2 px-2"
                            }
                        }
                        on:click=move |_evt| {
                            let new_show = !show_channel_add();
                            set_show_channel_add(new_show);
                            if new_show {
                                if let Some(input) = new_channel_ref() {
                                    let _ = input.focus();
                                }
                            }
                        }>
                        { move || if show_channel_add() { "X" } else { "+" } }
                    </button>
                </div>
                {add_channel_form}
                <div class="bg-emerald-900 grow mx-2 p-1 rounded-lg overflow-y-scroll">
                    <For
                        each=get_channel_list
                        key=|(cid, _)| *cid
                        let:child>
                        <DisplayChannel
                            channel_id=child.0
                            display_name=child.1 />
                    </For>
                </div>
            </div>
            <div class="h-1/2 flex flex-col">
                <h2 class="font-bold text-lg mx-2 pt-2 pl-2">Users</h2>
                <div class="grow bg-emerald-900 m-2 p-1 rounded-lg overflow-y-scroll">
                    <For
                        each=get_user_list
                        key=|(uid, _)| *uid
                        let:child>
                        <DisplayUser
                            user_id=child.0
                            username=child.1 />
                    </For>
                </div>
            </div>
        </div>
    }
}

#[component]
fn DisplayChannel(channel_id: ChannelId, display_name: String) -> impl IntoView {
    let mailroom: ReadSignal<Mailroom> = expect_context();
    let set_mailroom: WriteSignal<Mailroom> = expect_context();

    let get_css_class = move || {
        let mailroom = mailroom();
        if mailroom.is_active(channel_id) {
            "m-1 p-1 block rounded bg-emerald-700 text-neutral-950 font-medium"
        } else if mailroom.has_unread(channel_id) {
            "m-1 p-1 block rounded bg-amber-300 hover:bg-green-500 text-green-900 font-medium"
        } else {
            "m-1 p-1 block rounded hover:bg-emerald-950 text-amber-100"
        }
    };

    view! {
        <a  class={get_css_class}
            href={format!("#{}", display_name.clone())}
            on:click=move |evt| {
                evt.prevent_default(); // todo: history api?
                let mailroom = mailroom();
                mailroom.set_active(channel_id);
                set_mailroom(mailroom);
            }>
            #{display_name}
        </a>
    }
}

#[component]
fn DisplayUser(user_id: UserId, username: String) -> impl IntoView {
    let mailroom: ReadSignal<Mailroom> = expect_context();
    let set_mailroom: WriteSignal<Mailroom> = expect_context();

    let get_css_class = move || {
        let mailroom = mailroom();
        if mailroom.is_active(user_id) {
            "m-1 p-1 block rounded bg-emerald-700 text-white font-medium"
        } else if mailroom.has_unread(user_id) {
            "m-1 p-1 block rounded hover:bg-green-500 text-amber-100 font-bold bg-rose-700"
        } else {
            "m-1 p-1 block rounded hover:bg-emerald-950 text-amber-100"
        }
    };
    let display_username = username.clone();

    let get_display_name = move || {
        let mailroom = mailroom();
        match mailroom.current_user_id() {
            Some(id) if id == user_id => format!("{} (you)", display_username.clone()),
            _ => display_username.clone(),
        }
    };

    view! {
        <a class={get_css_class}
            href={format!("@{}", username.clone())}
            on:click=move |evt| {
                evt.prevent_default();
                let mailroom = mailroom();
                mailroom.set_active(user_id);
                set_mailroom(mailroom);
            }>
            {get_display_name}
        </a>
    }
}

#[component]
fn Chat() -> impl IntoView {
    let mailroom: ReadSignal<Mailroom> = expect_context();
    let set_mailroom: WriteSignal<Mailroom> = expect_context();

    create_effect(move |_| {
        register_handler(move |chat_msg: ChatMessage| {
            let mailroom = mailroom.get_untracked();
            mailroom.add_message(chat_msg);
            set_mailroom(mailroom);
        });
    });

    let active_messages = move || {
        let mailroom = mailroom();
        mailroom.active_messages()
    };

    let get_chat_title = move || {
        let mailroom = mailroom();
        mailroom.active_display_name()
    };

    view! {
        <div class="basis-3/4 overflow-hidden flex flex-col bg-green-950 rounded-r-md">
            <h1 class="mx-2 my-1 text-xl font-bold text-amber-300">{get_chat_title}</h1>
            <DisplayMessages messages={active_messages} />
            <ChatInput />
        </div>
    }
}

#[component]
fn DisplayMessages<F: Fn() -> Vec<ChatMessage> + Copy + 'static>(messages: F) -> impl IntoView {
    let (scrolled_bottom, set_scrolled_bottom) = create_signal(true);
    let messages_element: NodeRef<Div> = create_node_ref();

    create_effect(move |_| {
        messages(); // track on messages
                    // only do this once rendered
        if let Some(div) = messages_element() {
            let bottom = div.scroll_height() - div.client_height();
            let scroll_top = div.scroll_top();
            if scrolled_bottom.get_untracked() && scroll_top < bottom {
                div.set_scroll_top(bottom);
            }
        };
    });

    // todo: do flair properly

    // ¡WARNING! CSS is sometimes smoking something very strong.
    //           Below the `grow` AND some height i.e. `h-1` must be present
    //           `grow` makes the flexbox grow to fill the screen.
    //           `h-1` basically sets the minimum height to like 1 pixel (technically it sets `height: 0.25rem`)
    //           This might seem contradictory...
    //           Or perhaps one overrides the other, and thus `h-1` needs to be cleaned up and removed...
    //           You'd be wrong, like I was.
    //           Apparently both need to be present for `overflow-y-scroll` to take effect...
    // note: perhaps there's a better way to do this... but also the fact all three of these interact is very strange
    view! {
        <div class="m-2 mb-0 bg-emerald-900 grow h-1 text-amber-100 overflow-y-scroll scroll-p-0"
            on:scroll=move |_| {
                // mainly want to check if scrolled to the bottom
                // if scrolled to the bottom, then new messages should scroll you down too.
                // .unwrap() ok here bc we're literally in the element's scroll handler
                let div = messages_element.get_untracked().unwrap();
                let scroll_top = div.scroll_top();
                let bottom = div.scroll_height() - div.client_height();
                // if we're within 17 pixels of the bottom, consider us at the bottom
                if (bottom - scroll_top).abs() <= 17 {
                    set_scrolled_bottom(true);
                } else {
                    set_scrolled_bottom(false);
                }
            }
            node_ref=messages_element>
            <For
                each=messages
                key=|chat_msg| chat_msg.id.clone()
                let:child
            >
                <DisplayChatMessage chat=child />
            </For>
        </div>
    }
}

#[component]
fn DisplayChatMessage(chat: ChatMessage) -> impl IntoView {
    let mailroom: ReadSignal<Mailroom> = expect_context();
    let get_username_and_flair = move |from| {
        let mailroom = mailroom();
        let maybe_user = mailroom.get_user(from);
        if let Some(user) = maybe_user {
            (user.username, user.flair)
        } else if let UserId(0) = from {
            ("server".to_string(), Some("🖥️".to_string()))
        } else {
            ("unknown user".to_string(), Some("❓".to_string()))
        }
    };
    let get_datetime = move || Date::new(chat.ts).to_locale_string().to_string();
    let get_user_display = move || {
        let (username, flair) = get_username_and_flair(chat.from);
        view! {
            <div>
                <b>"[" {username} "] " {flair} </b>
                <span class="px-1 text-xs">{get_datetime}</span>
            </div>
        }
    };

    view! {
        <div class="m-1 p-1 flex flex-row">
            {get_user_display}
            <div> - {chat.content} </div>
        </div>
    }
}

#[component]
fn ChatInput() -> impl IntoView {
    let mailroom: ReadSignal<Mailroom> = expect_context();
    let (current_msg, set_current_msg) = create_signal("".to_string());

    let input_ref: NodeRef<Input> = create_node_ref();

    // whenever active mailroom changes, .focus() the input
    create_effect(move |_| {
        mailroom.get_untracked().set_active_hook(move || {
            if let Some(input) = input_ref.get_untracked() {
                let _ = input.focus();
            }
        });
    });

    view! {
        <form class="mx-2 mt-0"
            on:submit=move |evt| {
                evt.prevent_default();
                let msg = current_msg();
                if msg.len() > 0 {
                    let mailroom = mailroom();
                    let to = mailroom.active_selection();
                    logging::log!("Sending a message to: {to:?}");
                    let chat_msg = SendChatMessage {
                        to,
                        content: current_msg(),
                    };
                    send_message(chat_msg);
                    set_current_msg("".to_string());
                }
            }
        >
            <div class="flex flex-row py-3">
                <input class="p-2 mr-2 rounded w-full text-white bg-emerald-900"
                    type="text"
                    on:input=move |evt| {
                        set_current_msg(event_target_value(&evt).to_string());
                    }
                    prop:value={current_msg}
                    node_ref=input_ref
                />
                <button class="bg-green-500 hover:bg-green-700 text-white font-bold px-2 rounded">
                    Send
                </button>
            </div>
        </form>
    }
}
