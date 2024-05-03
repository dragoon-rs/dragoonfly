use futures::channel::mpsc::Sender;
use tokio::sync::Mutex;

use crate::commands::DragoonCommand;

pub(crate) struct AppState {
    pub sender: Mutex<Sender<DragoonCommand>>,
}

impl AppState {
    pub fn new(sender: Sender<DragoonCommand>) -> Self {
        AppState {
            sender: Mutex::new(sender),
        }
    }
}
