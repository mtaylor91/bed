use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::io::AsyncBufReadExt;
use tokio::task::JoinError;


#[derive(Debug)]
pub enum Error {
    CircularDependency,
    Exit(std::process::ExitStatus),
    Io(std::io::Error),
    JobFailed(Job),
    Join(JoinError),
    MissingDependency(String),
    Serde(serde_yml::Error),
    TaskFailed(Task),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::CircularDependency => write!(f, "Circular dependency detected"),
            Error::MissingDependency(name) => write!(f, "Missing dependency: {}", name),
            Error::JobFailed(job) => write!(f, "Job failed: {}", job.name),
            Error::Join(error) => write!(f, "Join error: {}", error),
            Error::TaskFailed(task) => write!(f, "Task failed: {}", task.name),
            Error::Exit(status) => write!(f, "Exit status: {}", status),
            Error::Io(error) => write!(f, "I/O error: {}", error),
            Error::Serde(error) => write!(f, "Serde error: {}", error),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::Io(error)
    }
}

impl From<serde_yml::Error> for Error {
    fn from(error: serde_yml::Error) -> Error {
        Error::Serde(error)
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Job {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub tasks: Vec<Task>,
}

impl Job {
    pub fn depends(&mut self, name: String) {
        self.depends.push(name);
    }

    pub fn new(name: String) -> Job {
        Job {
            name,
            depends: Vec::new(),
            tasks: Vec::new(),
        }
    }

    pub fn ready(&self, finished: &Vec<Job>) -> bool {
        self.depends.iter().all(|name| finished.iter().any(|job| job.name == *name))
    }

    pub async fn run(&mut self, tracker: TaskTracker) -> Result<(), Error> {
        // Check if all dependencies are available
        for task in &self.tasks {
            for name in &task.depends {
                if !self.tasks.iter().any(|task| task.name == *name) {
                    let name = format!("{}/{}", self.name, name);
                    return Err(Error::MissingDependency(name));
                }
            }
        }

        let mut pending = self.tasks.clone();
        let mut running = Vec::new();
        let mut finished = Vec::new();

        loop {
            // Filter out tasks that are ready to run
            pending.retain(|task| {
                // Check if the task is ready to run
                if task.ready(&finished) {
                    // Clone to avoid borrowing issues
                    let mut task = task.clone();
                    let task_name = task.name.clone();
                    let task_name2 = task.name.clone();
                    let task_name3 = task.name.clone();
                    let tracker_clone = tracker.clone();
                    let tracker_clone2 = tracker.clone();
                    // Spawn the task to run asynchronously
                    running.push(tokio::spawn(async move {
                        match task.run(StepTracker::new(task_name, tracker_clone)).await {
                            Ok(()) => {
                                tracker_clone2.modify(&task_name2, |task| {
                                    task.status = Status::Finished;
                                });
                                Ok(task)
                            }
                            Err(e) => {
                                tracker_clone2.modify(&task_name2, |task| {
                                    task.status = Status::Failed;
                                });
                                Err(e)
                            }
                        }
                    }));
                    // Update the task status
                    tracker.modify(&task_name3, |task| {
                        task.status = Status::Running;
                    });
                    // Remove the task from the pending list
                    false
                } else {
                    // Keep the task in the pending list
                    true
                }
            });


            if !running.is_empty() {
                // Wait for any task to finish
                let (done, _, rest) = futures::future::select_all(running).await;
                // Update the running list
                running = rest;
                // Match the result of the task
                match done {
                    Ok(Ok(task)) => {
                        // Add the task to the finished list
                        finished.push(task);
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(Error::Join(e));
                    }
                }
            } else if pending.is_empty() && running.is_empty() {
                self.tasks = finished;
                return Ok(());
            } else if running.is_empty() {
                return Err(Error::CircularDependency);
            }
        }
    }
}


#[derive(Clone)]
pub struct JobTracker {
    jobs: Arc<Mutex<HashMap<String, JobStatus>>>,
}

impl JobTracker {
    pub fn new() -> JobTracker {
        JobTracker {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, name: &str) -> Option<JobStatus> {
        self.jobs.lock().unwrap().get(name).cloned()
    }

    pub fn insert(&self, job: JobStatus) {
        self.jobs.lock().unwrap().insert(job.name.clone(), job);
    }

    pub fn modify<F>(&self, name: &str, f: F)
    where
        F: FnOnce(&mut JobStatus),
    {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(name) {
            f(job);
        }
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobStatus {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub tasks: Vec<TaskStatus>,
    #[serde(default)]
    pub status: Status,
}


pub struct Loader {
    pub directory: String,
    pub jobs: Vec<Job>,
}

impl Loader {
    pub fn new(directory: String) -> Loader {
        Loader {
            directory,
            jobs: Vec::new(),
        }
    }

    pub fn load(&mut self) -> Result<(), Error> {
        let entries = std::fs::read_dir(&self.directory)?;

        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_file() {
                        match path.extension() {
                            Some(ext) => {
                                if ext == "yml" || ext == "yaml" {
                                    self.load_file(path)?;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    return Err(Error::Io(e));
                }
            }
        }

        Ok(())
    }

    pub fn load_file(&mut self, path: std::path::PathBuf) -> Result<(), Error> {
        let file = std::fs::File::open(&path)?;
        let job = serde_yml::from_reader(file)?;
        self.jobs.push(job);
        Ok(())
    }

    pub fn runner(&self) -> Runner {
        let mut runner = Runner::new();
        runner.jobs = self.jobs.clone();
        runner
    }
}


pub struct Runner {
    pub jobs: Vec<Job>,
}

impl Runner {
    pub fn new() -> Runner {
        Runner {
            jobs: Vec::new(),
        }
    }

    pub async fn run(&mut self, tracker: JobTracker) -> Result<(), Error> {
        for job in &self.jobs {
            // Check if all dependencies are available
            for name in &job.depends {
                if !self.jobs.iter().any(|job| job.name == *name) {
                    return Err(Error::MissingDependency(name.clone()));
                }
            }

            // Create a job status
            tracker.insert(JobStatus {
                name: job.name.clone(),
                depends: job.depends.clone(),
                tasks: job.tasks.iter().map(|task| TaskStatus {
                    name: task.name.clone(),
                    depends: task.depends.clone(),
                    steps: task.steps.iter().map(|step| match step {
                        Step::Command { args } => StepStatus::Command {
                            args: args.clone(),
                            output: Vec::new(),
                            status: Status::Pending,
                        },
                    }).collect(),
                    status: Status::Pending,
                }).collect(),
                status: Status::Pending,
            });
        }

        let mut pending = self.jobs.clone();
        let mut running = Vec::new();
        let mut finished = Vec::new();

        loop {
            // Filter out jobs that are ready to run
            pending.retain(|job| {
                // Check if the job is ready to run
                if job.ready(&finished) {
                    // Clone to avoid borrowing issues
                    let mut job = job.clone();
                    let job_name = job.name.clone();
                    let job_name2 = job.name.clone();
                    let job_name3 = job.name.clone();
                    let tracker_clone = tracker.clone();
                    let tracker_clone2 = tracker.clone();
                    // Spawn the job to run asynchronously
                    running.push(tokio::spawn(async move {
                        match job.run(TaskTracker::new(job_name, tracker_clone)).await {
                            Ok(()) => {
                                tracker_clone2.modify(&job_name2, |job| {
                                    job.status = Status::Finished;
                                });
                                Ok(job)
                            }
                            Err(e) => {
                                tracker_clone2.modify(&job_name2, |job| {
                                    job.status = Status::Failed;
                                });
                                Err(e)
                            }
                        }
                    }));
                    // Update the job status
                    tracker.modify(&job_name3, |job| {
                        job.status = Status::Running;
                    });
                    // Remove the job from the pending list
                    false
                } else {
                    // Keep the job in the pending list
                    true
                }
            });

            if !running.is_empty() {
                // Wait for any job to finish
                let (done, _, rest) = futures::future::select_all(running).await;
                // Update the running list
                running = rest;
                // Match the result of the job
                match done {
                    Ok(Ok(job)) => {
                        // Add the job to the finished list
                        finished.push(job);
                    }
                    Ok(Err(e)) => {
                        return Err(e);
                    }
                    Err(e) => {
                        return Err(Error::Join(e));
                    }
                }
            } else if pending.is_empty() && running.is_empty() {
                self.jobs = finished;
                return Ok(());
            } else if running.is_empty() {
                return Err(Error::CircularDependency);
            }
        }
    }
}


#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Status {
    Pending,
    Running,
    Finished,
    Failed,
}

impl Default for Status {
    fn default() -> Status {
        Status::Pending
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Step {
    Command{args: Vec<String>},
}

impl Step {
    pub fn command(args: Vec<String>) -> Step {
        Step::Command { args }
    }

    pub async fn run(&mut self, index: usize, tracker: StepTracker) -> Result<(), Error> {
        match self {
            Step::Command { args } => {
                tracker.modify(index, |step| {
                    match step {
                        StepStatus::Command { status, .. } => {
                            *status = Status::Running;
                        }
                    }
                });

                let mut child = tokio::process::Command::new(&args[0])
                    .args(&args[1..])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?;

                let stdout = child.stdout.take().unwrap();
                let tracker_clone = tracker.clone();
                tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(stdout);
                    let mut buffer = String::new();
                    while reader.read_line(&mut buffer).await.unwrap() > 0 {
                        tracker_clone.log(index, &buffer);
                        buffer.clear();
                    }
                });

                let stderr = child.stderr.take().unwrap();
                let tracker_clone = tracker.clone();
                tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(stderr);
                    let mut buffer = String::new();
                    while reader.read_line(&mut buffer).await.unwrap() > 0 {
                        tracker_clone.log(index, &buffer);
                        buffer.clear();
                    }
                });

                let status = child.wait().await?;
                if status.success() {
                    tracker.modify(index, |step| {
                        match step {
                            StepStatus::Command { status, .. } => {
                                *status = Status::Finished;
                            }
                        }
                    });

                    Ok(())
                } else {
                    tracker.modify(index, |step| {
                        match step {
                            StepStatus::Command { status, .. } => {
                                *status = Status::Failed;
                            }
                        }
                    });

                    Err(Error::Exit(status))
                }
            }
        }
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum StepStatus {
    Command{
        args: Vec<String>,
        output: Vec<String>,
        status: Status
    },
}


#[derive(Clone)]
pub struct StepTracker {
    task_name: String,
    task_tracker: TaskTracker,
}

impl StepTracker {
    pub fn new(task_name: String, task_tracker: TaskTracker) -> StepTracker {
        StepTracker {
            task_name,
            task_tracker,
        }
    }

    pub fn get(&self, index: usize) -> Option<StepStatus> {
        match self.task_tracker.get(&self.task_name) {
            Some(task) => task.steps.get(index).cloned(),
            None => None,
        }
    }

    pub fn log(&self, index: usize, message: &str) {
        print!("{}/{}: {}", self.task_tracker.job_name, self.task_name, message);
        self.modify(index, |step| {
            match step {
                StepStatus::Command { output, .. } => {
                    output.push(message.to_string());
                }
            }
        });
    }

    pub fn modify<F>(&self, index: usize, f: F)
    where
        F: FnOnce(&mut StepStatus),
    {
        self.task_tracker.modify(&self.task_name, |task| {
            if let Some(step) = task.steps.get_mut(index) {
                f(step);
            }
        });
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub steps: Vec<Step>,
}

impl Task {
    pub fn ready(&self, finished: &Vec<Task>) -> bool {
        self.depends.iter().all(|name| finished.iter().any(|task| task.name == *name))
    }

    pub async fn run(&mut self, tracker: StepTracker) -> Result<(), Error> {
        for (index, step) in &mut self.steps.iter_mut().enumerate() {
            step.run(index, tracker.clone()).await?
        }

        Ok(())
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskStatus {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub steps: Vec<StepStatus>,
    #[serde(default)]
    pub status: Status,
}


#[derive(Clone)]
pub struct TaskTracker {
    job_name: String,
    job_tracker: JobTracker,
}

impl TaskTracker {
    pub fn new(job_name: String, job_tracker: JobTracker) -> TaskTracker {
        TaskTracker {
            job_name,
            job_tracker,
        }
    }

    pub fn get(&self, name: &str) -> Option<TaskStatus> {
        match self.job_tracker.get(&self.job_name) {
            Some(job) => job.tasks.iter().find(|task| task.name == name).cloned(),
            None => None,
        }
    }

    pub fn modify<F>(&self, name: &str, f: F)
    where
        F: FnOnce(&mut TaskStatus),
    {
        self.job_tracker.modify(&self.job_name, |job| {
            if let Some(task) = job.tasks.iter_mut().find(|task| task.name == name) {
                f(task);
            }
        });
    }
}
