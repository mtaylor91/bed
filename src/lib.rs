use serde::Deserialize;
use tokio::io::AsyncBufReadExt;


#[derive(Debug)]
pub enum Error {
    CircularDependency,
    Exit(std::process::ExitStatus),
    Io(std::io::Error),
    JobFailed(Job),
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


#[derive(Clone, Debug, Deserialize)]
pub struct Job {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub status: Status,
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
            status: Status::Pending,
        }
    }

    pub fn ready(&self, finished: &Vec<Job>) -> bool {
        self.depends.iter().all(|name| {
            finished.iter().any(|job|
                job.name == *name && job.status == Status::Finished
            )
        })
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        self.status = Status::Running;

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
                    // Clone the task to avoid borrowing issues
                    let mut task = task.clone();
                    // Spawn the task to run asynchronously
                    running.push(tokio::spawn(async move {
                        match task.run().await {
                            Ok(()) => {}
                            Err(e) => {
                                eprintln!("Error: {}", e);
                            }
                        }
                        task
                    }));
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
                    Ok(task) => {
                        if task.status == Status::Failed {
                            // Return an error if the task failed
                            return Err(Error::TaskFailed(task))
                        } else {
                            // Add the task to the finished list
                            finished.push(task);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: {:?}", e);
                    }
                }
            } else if pending.is_empty() && running.is_empty() {
                self.tasks = finished;
                self.status = Status::Finished;
                return Ok(());
            } else if running.is_empty() {
                return Err(Error::CircularDependency);
            }
        }
    }
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
    pub status: Status,
}

impl Runner {
    pub fn new() -> Runner {
        Runner {
            jobs: Vec::new(),
            status: Status::Pending,
        }
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        self.status = Status::Running;

        // Check if all dependencies are available
        for job in &self.jobs {
            for name in &job.depends {
                if !self.jobs.iter().any(|job| job.name == *name) {
                    return Err(Error::MissingDependency(name.clone()));
                }
            }
        }

        let mut pending = self.jobs.clone();
        let mut running = Vec::new();
        let mut finished = Vec::new();

        loop {
            // Filter out jobs that are ready to run
            pending.retain(|job| {
                // Check if the job is ready to run
                if job.ready(&finished) {
                    // Clone the job to avoid borrowing issues
                    let mut job = job.clone();
                    // Spawn the job to run asynchronously
                    running.push(tokio::spawn(async move {
                        match job.run().await {
                            Ok(()) => {}
                            Err(e) => {
                                eprintln!("Error: {}", e);
                            }
                        }
                        job
                    }));
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
                    Ok(job) => {
                        if job.status == Status::Failed {
                            // Return an error if the job failed
                            return Err(Error::JobFailed(job))
                        } else {
                            // Add the job to the finished list
                            finished.push(job);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: {:?}", e);
                    }
                }
            } else if pending.is_empty() && running.is_empty() {
                self.jobs = finished;
                self.status = Status::Finished;
                return Ok(());
            } else if running.is_empty() {
                return Err(Error::CircularDependency);
            }
        }
    }

    pub fn job(&mut self, name: String) -> &mut Job {
        let job = Job {
            name,
            depends: Vec::new(),
            tasks: Vec::new(),
            status: Status::Pending,
        };
        self.jobs.push(job);
        self.jobs.last_mut().unwrap()
    }
}


#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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


#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Step {
    Command{args: Vec<String>},
}

impl Step {
    pub fn command(args: Vec<String>) -> Step {
        Step::Command { args }
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        match self {
            Step::Command { args } => {
                let mut child = tokio::process::Command::new(&args[0])
                    .args(&args[1..])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?;

                let stdout = child.stdout.take().unwrap();
                tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(stdout);
                    let mut buffer = String::new();
                    while reader.read_line(&mut buffer).await.unwrap() > 0 {
                        print!("{}", buffer);
                        buffer.clear();
                    }
                });

                let stderr = child.stderr.take().unwrap();
                tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(stderr);
                    let mut buffer = String::new();
                    while reader.read_line(&mut buffer).await.unwrap() > 0 {
                        eprint!("{}", buffer);
                        buffer.clear();
                    }
                });

                let status = child.wait().await?;
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::Exit(status))
                }
            }
        }
    }
}


#[derive(Clone, Debug, Deserialize)]
pub struct Task {
    pub name: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub status: Status,
}

impl Task {
    pub fn ready(&self, finished: &Vec<Task>) -> bool {
        self.depends.iter().all(|name| {
            finished.iter().any(|task|
                task.name == *name && task.status == Status::Finished
            )
        })
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        self.status = Status::Running;

        for step in &mut self.steps {
            match step.run().await {
                Ok(()) => {}
                Err(e) => {
                    self.status = Status::Failed;
                    return Err(e);
                }
            }
        }

        self.status = Status::Finished;

        Ok(())
    }
}
