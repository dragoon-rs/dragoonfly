use tokio::sync::mpsc::UnboundedSender;

use crate::commands::DragoonCommand;

pub(crate) struct AppState {
    pub cmd_sender: UnboundedSender<DragoonCommand>,
}

impl AppState {
    pub fn new(cmd_sender: UnboundedSender<DragoonCommand>) -> Self {
        AppState { cmd_sender }
    }
}
