# -*- coding: utf-8 -*-
# task/manager.rs: batch aggregation (T-20/T-05)
import io, sys

path = r"src-tauri/src/task/manager.rs"
text = io.open(path, "r", encoding="utf-8", newline="").read()
NL = "\r\n" if "\r\n" in text else "\n"
Q = chr(34)

def q(s):
    return Q + s + Q

def rep(tag, old, new):
    global text
    n = text.count(old)
    if n != 1:
        print("FAIL[%s](%d)" % (tag, n))
        sys.exit(1)
    text = text.replace(old, new, 1)

# 1. imports + struct fields
rep("imports",
    "use crate::models::task::{Task, TaskRequest, TaskStatus};",
    "use crate::models::task::{BatchState, Task, TaskRequest, TaskStatus};")

rep("fields",
    "    /// Shared DB connection for task persistence (history + crash recovery)." + NL
    + "    db: Arc<Mutex<Connection>>," + NL
    + "}",
    "    /// Shared DB connection for task persistence (history + crash recovery)." + NL
    + "    db: Arc<Mutex<Connection>>," + NL
    + "    /// Aggregate state of multi-repo batches (T-20), keyed by batch id." + NL
    + "    batches: Arc<DashMap<String, BatchState>>," + NL
    + "    /// Kept to emit progress events for synthetic batch tasks." + NL
    + "    app_handle: AppHandle," + NL
    + "}")

# 2. new(): batches map + pass to worker pool + store app_handle
rep("new",
    "        let (sender, receiver) = queue::new_queue(128);" + NL
    + "        let active_tasks = Arc::new(DashMap::<String, Task>::new());" + NL
    + "        let cancel_flags = Arc::new(DashMap::<String, Arc<AtomicBool>>::new());" + NL,
    "        let (sender, receiver) = queue::new_queue(128);" + NL
    + "        let active_tasks = Arc::new(DashMap::<String, Task>::new());" + NL
    + "        let cancel_flags = Arc::new(DashMap::<String, Arc<AtomicBool>>::new());" + NL
    + "        let batches = Arc::new(DashMap::<String, BatchState>::new());" + NL)

rep("new2",
    "            Arc::clone(&cancel_flags)," + NL
    + "            app_handle," + NL
    + "            Arc::clone(&db)," + NL
    + "        );",
    "            Arc::clone(&cancel_flags)," + NL
    + "            app_handle.clone()," + NL
    + "            Arc::clone(&db)," + NL
    + "            Arc::clone(&batches)," + NL
    + "        );")

rep("new3",
    "        TaskManager {" + NL
    + "            sender," + NL
    + "            active_tasks," + NL
    + "            cancel_flags," + NL
    + "            db," + NL
    + "        }",
    "        TaskManager {" + NL
    + "            sender," + NL
    + "            active_tasks," + NL
    + "            cancel_flags," + NL
    + "            db," + NL
    + "            batches," + NL
    + "            app_handle," + NL
    + "        }")

# 3. submit(): batch creation + children batch_id + queue-failure accounting
rep("submit",
    "    pub fn submit(&self, requests: &[TaskRequest]) -> AppResult<Vec<String>> {" + NL
    + "        let mut ids = Vec::with_capacity(requests.len());" + NL
    + NL
    + "        for req in requests {" + NL
    + "            let id = Uuid::new_v4().to_string();" + NL
    + "            let now = Utc::now().to_rfc3339();" + NL
    + NL
    + "            let task = Task {" + NL
    + "                id: id.clone()," + NL
    + "                task_type: req.task_type.clone()," + NL
    + "                repo_path: req.repo_path.clone()," + NL
    + "                repo_name: req.repo_name.clone()," + NL
    + "                status: TaskStatus::Queued," + NL
    + "                created_at: now," + NL
    + "            };",
    "    pub fn submit(&self, requests: &[TaskRequest]) -> AppResult<Vec<String>> {" + NL
    + "        let mut ids = Vec::with_capacity(requests.len());" + NL
    + NL
    + "        // Multi-repo submits get a synthetic batch task (T-20): it tracks" + NL
    + "        // the aggregate (Partial Success etc.) while children keep their" + NL
    + "        // per-repo rows. Single submits stay flat (no batch row)." + NL
    + "        let batch_id = (requests.len() > 1).then(|| Uuid::new_v4().to_string());" + NL
    + "        if let Some(bid) = &batch_id {" + NL
    + "            let now = Utc::now().to_rfc3339();" + NL
    + "            let batch_task = Task {" + NL
    + "                id: bid.clone()," + NL
    + "                task_type: requests[0].task_type.clone()," + NL
    + "                repo_path: String::new()," + NL
    + "                repo_name: format!(" + Q + "批量（{} 个仓库）" + Q + ", requests.len())," + NL
    + "                status: TaskStatus::Running { progress: 0.0 }," + NL
    + "                created_at: now," + NL
    + "                batch_id: None," + NL
    + "            };" + NL
    + "            let row_id = self.persist_new_task(&batch_task);" + NL
    + "            self.batches.insert(" + NL
    + "                bid.clone()," + NL
    + "                BatchState {" + NL
    + "                    task: batch_task.clone()," + NL
    + "                    db_row_id: row_id.unwrap_or(0)," + NL
    + "                    total: requests.len()," + NL
    + "                    finished: 0," + NL
    + "                    succeeded: 0," + NL
    + "                    failed: 0," + NL
    + "                    cancelled: 0," + NL
    + "                }," + NL
    + "            );" + NL
    + "            worker::emit_progress(&self.app_handle, &batch_task);" + NL
    + "        }" + NL
    + NL
    + "        for req in requests {" + NL
    + "            let id = Uuid::new_v4().to_string();" + NL
    + "            let now = Utc::now().to_rfc3339();" + NL
    + NL
    + "            let task = Task {" + NL
    + "                id: id.clone()," + NL
    + "                task_type: req.task_type.clone()," + NL
    + "                repo_path: req.repo_path.clone()," + NL
    + "                repo_name: req.repo_name.clone()," + NL
    + "                status: TaskStatus::Queued," + NL
    + "                created_at: now," + NL
    + "                batch_id: batch_id.clone()," + NL
    + "            };")

# 4. queue-failure path: account the child into the batch before returning
rep("queuefail",
    "            if let Err(e) = self.sender.try_send(TaskMessage { task }) {" + NL
    + "                // Remove from active tasks if sending failed and mark the" + NL
    + "                // persisted record failed so crash recovery won't resurrect it." + NL
    + "                self.active_tasks.remove(&id);" + NL
    + "                self.cancel_flags.remove(&id);" + NL
    + "                self.persist_task_status(" + NL
    + "                    &id," + NL
    + "                    TaskStatus::Failed { error: e.to_string() }.key()," + NL
    + "                );" + NL
    + "                return Err(AppError::Task(format!(" + NL
    + "                    " + Q + "Failed to queue task: {}" + Q + "," + NL
    + "                    e" + NL
    + "                )));" + NL
    + "            }",
    "            if let Err(e) = self.sender.try_send(TaskMessage { task: task.clone() }) {" + NL
    + "                // Remove from active tasks if sending failed and mark the" + NL
    + "                // persisted record failed so crash recovery won't resurrect it." + NL
    + "                self.active_tasks.remove(&id);" + NL
    + "                self.cancel_flags.remove(&id);" + NL
    + "                let failed = TaskStatus::Failed { error: e.to_string() };" + NL
    + "                self.persist_task_status(&id, failed.key());" + NL
    + "                // Account the failed child into its batch so the batch" + NL
    + "                // cannot hang unfinished (T-20 aggregation)." + NL
    + "                if task.batch_id.is_some() {" + NL
    + "                    let mut failed_task = task;" + NL
    + "                    failed_task.status = failed;" + NL
    + "                    worker::update_batch(&self.batches, &self.db, &self.app_handle, &failed_task);" + NL
    + "                }" + NL
    + "                return Err(AppError::Task(format!(" + NL
    + "                    " + Q + "Failed to queue task: {}" + Q + "," + NL
    + "                    e" + NL
    + "                )));" + NL
    + "            }")

# 5. persist_new_task returns the row id
rep("persist",
    "    fn persist_new_task(&self, task: &Task) {" + NL
    + "        let Ok(conn) = self.db.lock() else {" + NL
    + "            return;" + NL
    + "        };",
    "    fn persist_new_task(&self, task: &Task) -> Option<i64> {" + NL
    + "        let Ok(conn) = self.db.lock() else {" + NL
    + "            return None;" + NL
    + "        };")

rep("persist2",
    "        if let Err(e) = dao::insert_task_record(" + NL
    + "            &conn," + NL
    + "            &task.id," + NL
    + "            &task_type_json," + NL
    + "            task.status.key()," + NL
    + "            &params_json," + NL
    + "            &task.created_at," + NL
    + "        ) {" + NL
    + "            log::warn!(" + Q + "Failed to persist task {}: {}" + Q + ", task.id, e);" + NL
    + "        }" + NL
    + "    }",
    "        match dao::insert_task_record(" + NL
    + "            &conn," + NL
    + "            &task.id," + NL
    + "            &task_type_json," + NL
    + "            task.status.key()," + NL
    + "            &params_json," + NL
    + "            &task.created_at," + NL
    + "        ) {" + NL
    + "            Ok(row_id) => Some(row_id)," + NL
    + "            Err(e) => {" + NL
    + "                log::warn!(" + Q + "Failed to persist task {}: {}" + Q + ", task.id, e);" + NL
    + "                None" + NL
    + "            }" + NL
    + "        }" + NL
    + "    }")

with io.open(path, "w", encoding="utf-8", newline="") as f:
    f.write(text)
print("OK manager")
