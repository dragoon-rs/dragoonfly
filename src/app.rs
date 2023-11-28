#[cfg(feature = "file-sharing")]
use futures::channel::mpsc::Receiver;
use futures::channel::mpsc::Sender;
use tokio::sync::Mutex;

use crate::commands::DragoonCommand;
#[cfg(feature = "file-sharing")]
use crate::dragoon_network::DragoonEvent;

pub(crate) struct AppState {
    pub sender: Mutex<Sender<DragoonCommand>>,
    #[cfg(feature = "file-sharing")]
    pub event_receiver: Mutex<Receiver<DragoonEvent>>,
}

impl AppState {
    pub fn new(
        sender: Sender<DragoonCommand>,
        #[cfg(feature = "file-sharing")] event_receiver: Receiver<DragoonEvent>,
    ) -> Self {
        AppState {
            sender: Mutex::new(sender),
            #[cfg(feature = "file-sharing")]
            event_receiver: Mutex::new(event_receiver),
        }
    }
}
