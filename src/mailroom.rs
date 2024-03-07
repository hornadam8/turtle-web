use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use turtle_protocol::{
    Channel, ChannelId, ChannelsInfo, ChatMessage, SendableId, User, UserId, UsersInfo,
};

#[derive(Clone, Debug)]
struct Mailbox {
    display_name: Rc<RefCell<String>>,
    has_unread: Rc<RefCell<bool>>,
    is_active: Rc<RefCell<bool>>,
    messages: Rc<RefCell<Vec<ChatMessage>>>,
}

impl Mailbox {
    fn new(display_name: String) -> Self {
        Self {
            display_name: Rc::new(RefCell::new(display_name)),
            has_unread: Rc::new(RefCell::new(false)),
            is_active: Rc::new(RefCell::new(false)),
            messages: Rc::new(RefCell::new(vec![])),
        }
    }

    fn get_display_name(&self) -> String {
        self.display_name.borrow().clone()
    }

    fn add_message(&self, msg: ChatMessage) {
        let mut messages = self.messages.borrow_mut();
        messages.push(msg);
        if !*self.is_active.borrow() {
            *self.has_unread.borrow_mut() = true;
        } // else has_unread = false ?
    }

    fn set_active(&self) {
        *self.is_active.borrow_mut() = true;
        *self.has_unread.borrow_mut() = false;
    }

    fn set_inactive(&self) {
        *self.is_active.borrow_mut() = false;
    }

    fn is_active(&self) -> bool {
        *self.is_active.borrow()
    }

    fn has_unread(&self) -> bool {
        *self.has_unread.borrow()
    }

    fn get_messages(&self) -> Vec<ChatMessage> {
        self.messages.borrow().clone()
    }
}

#[derive(Clone)]
pub struct Mailroom {
    active_id: Rc<RefCell<SendableId>>,
    active_hook: Rc<RefCell<Option<Box<dyn FnMut() + 'static>>>>,
    current_user_id: Rc<RefCell<Option<UserId>>>,
    mailboxes: Rc<RefCell<HashMap<SendableId, Mailbox>>>,
    users: Rc<RefCell<HashMap<UserId, User>>>,
}

impl Mailroom {
    pub fn new(active_id: impl Into<SendableId>) -> Self {
        // todo: use Default to shorten this?
        Self {
            active_id: Rc::new(RefCell::new(active_id.into())),
            active_hook: Rc::new(RefCell::new(None)),
            current_user_id: Rc::new(RefCell::new(None)),
            mailboxes: Rc::new(RefCell::new(HashMap::new())),
            users: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn set_current_user_id(&self, user_id: UserId) {
        *self.current_user_id.borrow_mut() = Some(user_id);
    }

    pub fn current_user_id(&self) -> Option<UserId> {
        let current_user_id = self.current_user_id.borrow();
        current_user_id.clone()
    }

    pub fn add_channels(&self, info: ChannelsInfo) {
        let mut mailboxes = self.mailboxes.borrow_mut();
        let channels = info.channels;
        for channel in channels {
            let sid = channel.id.into();
            // if mailbox doesn't exist, make one
            mailboxes.entry(sid).or_insert(Mailbox::new(channel.name));
        }
    }

    pub fn add_channel(&self, channel: Channel) {
        let mut mailboxes = self.mailboxes.borrow_mut();
        let sid = channel.id.into();
        mailboxes.entry(sid).or_insert(Mailbox::new(channel.name));
    }

    pub fn add_users(&self, info: UsersInfo) {
        let mut mailboxes = self.mailboxes.borrow_mut();
        let mut users = self.users.borrow_mut();
        let new_users = info.users;
        for user in new_users {
            let sid = user.id.into();
            mailboxes
                .entry(sid)
                .or_insert(Mailbox::new(user.username.clone()));
            users.entry(user.id).or_insert(user);
        }
    }

    pub fn add_user(&self, user: User) {
        let mut mailboxes = self.mailboxes.borrow_mut();
        let mut users = self.users.borrow_mut();
        let sid = user.id.into();
        mailboxes.insert(sid, Mailbox::new(user.username.clone()));
        users.insert(user.id, user);
    }

    pub fn remove_user(&self, user_id: UserId) {
        let mut mailboxes = self.mailboxes.borrow_mut();
        let mut users = self.users.borrow_mut();
        mailboxes.remove(&user_id.into());
        users.remove(&user_id);
    }

    pub fn get_user(&self, user_id: UserId) -> Option<User> {
        self.users.borrow().get(&user_id).cloned()
    }

    pub fn add_message(&self, msg: ChatMessage) {
        let mut mailboxes = self.mailboxes.borrow_mut();
        // handle DMs, kinda tricky
        let mailbox_id = match msg.to {
            SendableId::U(uid) => {
                let current_user_id = self.current_user_id.borrow();
                match current_user_id.as_ref() {
                    Some(id) if *id == uid => msg.from.into(),
                    _ => uid.into(),
                }
            }
            channel_to => channel_to,
        };

        let entry = mailboxes
            .entry(mailbox_id)
            .or_insert(Mailbox::new("unknown".to_string()));
        entry.add_message(msg);
    }

    pub fn channel_list(&self) -> Vec<(ChannelId, String)> {
        let mailboxes = self.mailboxes.borrow();
        let mut list: Vec<_> = mailboxes
            .iter()
            .filter(|(sid, _)| sid.is_channel())
            .map(|(sid, mb)| match sid {
                SendableId::C(cid) => (*cid, mb.display_name.borrow().clone()),
                _ => unreachable!("has to be a channel"),
            })
            .collect();
        list.sort_by(|a, b| a.1.cmp(&b.1));
        list
    }

    pub fn user_list(&self) -> Vec<(UserId, String)> {
        let users = self.users.borrow();
        let current_user_id = *self.current_user_id.borrow();
        let mut list: Vec<_> = users
            .iter()
            .map(|(uid, user)| (*uid, user.username.clone()))
            .collect();

        list.sort_by(|a, b| {
            // sorting users is slightly trickier, want your current user at the top, all else alphabetical
            if let Some(current_uid) = current_user_id {
                if a.0 == current_uid {
                    std::cmp::Ordering::Less
                } else if b.0 == current_uid {
                    std::cmp::Ordering::Greater
                } else {
                    a.1.cmp(&b.1)
                }
            } else {
                // we don't know which user we are, so just normal sort
                a.1.cmp(&b.1)
            }
        });
        list
    }

    pub fn active_selection(&self) -> SendableId {
        *self.active_id.borrow()
    }

    pub fn active_messages(&self) -> Vec<ChatMessage> {
        let active_id = self.active_id.borrow();
        let mailboxes = self.mailboxes.borrow();
        mailboxes
            .get(&*active_id)
            .map(|mb| mb.get_messages())
            .unwrap_or(vec![])
    }

    pub fn active_display_name(&self) -> Option<String> {
        let active_id = self.active_id.borrow();
        let mailboxes = self.mailboxes.borrow();
        if let Some(mailbox) = mailboxes.get(&*active_id) {
            if active_id.is_user() {
                return Some(format!("@{}", mailbox.get_display_name()));
            } else {
                return Some(format!("#{}", mailbox.get_display_name()));
            }
        }
        None
    }

    pub fn set_active(&self, id: impl Into<SendableId>) {
        let mailbox_id = id.into();
        let mut active_id = self.active_id.borrow_mut();
        let mailboxes = self.mailboxes.borrow();
        // tell the old mailbox no longer active
        if let Some(mailbox) = mailboxes.get(&*active_id) {
            mailbox.set_inactive()
        }
        // update mailroom
        *active_id = mailbox_id;
        // update the new mailbox
        if let Some(mailbox) = mailboxes.get(&*active_id) {
            mailbox.set_active()
        }
        // also call the hook if it's set
        let mut hook = self.active_hook.borrow_mut();
        if let Some(mut box_f) = hook.take() {
            box_f();
            // then put it back
            *hook = Some(box_f);
        }
    }

    pub fn set_active_hook(&self, f: impl FnMut() + 'static) {
        *self.active_hook.borrow_mut() = Some(Box::new(f));
    }

    pub fn is_active(&self, id: impl Into<SendableId>) -> bool {
        let mailbox_id = id.into();
        let mailboxes = self.mailboxes.borrow();
        mailboxes
            .get(&mailbox_id)
            .map(|mb| mb.is_active())
            .unwrap_or(false)
    }

    pub fn has_unread(&self, id: impl Into<SendableId>) -> bool {
        let mailbox_id = id.into();
        let mailboxes = self.mailboxes.borrow();
        mailboxes
            .get(&mailbox_id)
            .map(|mb| mb.has_unread())
            .unwrap_or(false)
    }
}
