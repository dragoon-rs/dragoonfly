use futures::channel::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;

use crate::{commands::DragoonCommand, dragoon_network::DragoonEvent};

pub(crate) struct AppState {
    pub sender: Mutex<Sender<DragoonCommand>>,
    pub event_receiver: Mutex<Receiver<DragoonEvent>>,
}

impl AppState {
    pub fn new(sender: Sender<DragoonCommand>, event_receiver: Receiver<DragoonEvent>) -> Self {
        AppState {
            sender: Mutex::new(sender),
            event_receiver: Mutex::new(event_receiver),
        }
    }
}
