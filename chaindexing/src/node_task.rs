pub struct NodeTask {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl NodeTask {
    pub fn new(tasks: Vec<tokio::task::JoinHandle<()>>) -> Self {
        NodeTask { tasks }
    }
    pub fn stop(&self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
