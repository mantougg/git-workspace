use tokio::sync::mpsc;

use crate::models::task::Task;

/// Wrapper for a task sent through the mpsc channel.
pub struct TaskMessage {
    pub task: Task,
}

/// Create a new task queue (mpsc channel) with the given buffer size.
///
/// Returns the sender (for submitting tasks) and receiver (for workers).
pub fn new_queue(buffer_size: usize) -> (mpsc::Sender<TaskMessage>, mpsc::Receiver<TaskMessage>) {
    mpsc::channel::<TaskMessage>(buffer_size)
}
