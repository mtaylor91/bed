
pub struct Job {
    pub name: String,
    pub depends: Vec<String>,
    pub tasks: Vec<Task>,
    pub status: Status,
}

impl Clone for Job {
    fn clone(&self) -> Job {
        Job {
            name: self.name.clone(),
            depends: self.depends.clone(),
            tasks: self.tasks.clone(),
            status: self.status.clone(),
        }
    }
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

    pub async fn run(mut self) -> Job {
        if self.status != Status::Pending {
            return self;
        }

        self.status = Status::Running;

        for task in &mut self.tasks {
            task.run().await;
        }

        self.status = Status::Finished;

        self
    }

    pub fn task(&mut self) -> &mut Task {
        let task = Task {
            steps: Vec::new(),
            status: Status::Pending,
        };
        self.tasks.push(task);
        self.tasks.last_mut().unwrap()
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

    pub async fn run(&mut self) {
        if self.status != Status::Pending {
            return;
        }

        self.status = Status::Running;

        let mut pending = self.jobs.clone();
        let mut running = Vec::new();
        let mut finished = Vec::new();

        while !pending.is_empty() || !running.is_empty() {
            pending.retain(|job| {
                if job.ready(&finished) {
                    let job = job.clone();
                    running.push(tokio::spawn(async move {
                        job.run().await
                    }));
                    false
                } else {
                    true
                }
            });

            if !running.is_empty() {
                let (done, _, rest) = futures::future::select_all(running).await;
                running = rest;

                match done {
                    Ok(job) => {
                        finished.push(job);
                    }
                    Err(e) => {
                        eprintln!("Error: {:?}", e);
                    }
                }
            } else if pending.is_empty() && running.is_empty() {
                break;
            } else if running.is_empty() {
                eprintln!("Error: Circular dependency detected");
                break;
            }
        }

        self.jobs = finished;
        self.status = Status::Finished;
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


pub enum Spec {
    Command{args: Vec<String>},
}

impl Clone for Spec {
    fn clone(&self) -> Spec {
        match self {
            Spec::Command { args } => Spec::Command {
                args: args.clone(),
            },
        }
    }
}

impl Spec {
    pub fn command(args: Vec<String>) -> Spec {
        Spec::Command { args }
    }
}


pub enum Status {
    Pending,
    Running,
    Finished,
    Failed,
}

impl Clone for Status {
    fn clone(&self) -> Status {
        match self {
            Status::Pending => Status::Pending,
            Status::Running => Status::Running,
            Status::Finished => Status::Finished,
            Status::Failed => Status::Failed,
        }
    }
}

impl PartialEq for Status {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Status::Pending, Status::Pending) => true,
            (Status::Running, Status::Running) => true,
            (Status::Finished, Status::Finished) => true,
            (Status::Failed, Status::Failed) => true,
            _ => false,
        }
    }
}


pub struct Step {
    pub spec: Spec,
    pub status: Status,
}

impl Clone for Step {
    fn clone(&self) -> Step {
        Step {
            spec: self.spec.clone(),
            status: self.status.clone(),
        }
    }
}

impl Step {
    pub fn new(spec: Spec) -> Step {
        Step {
            spec,
            status: Status::Pending,
        }
    }

    pub async fn run(&mut self) {
        if self.status != Status::Pending {
            return;
        }

        self.status = Status::Running;

        match &self.spec {
            Spec::Command { args } => {
                let mut command = tokio::process::Command::new(&args[0]);
                for arg in &args[1..] {
                    command.arg(arg);
                }

                let status = command.status().await;
                match status {
                    Ok(status) => {
                        if status.success() {
                            self.status = Status::Finished;
                        } else {
                            self.status = Status::Failed;
                        }
                    }
                    Err(_) => {
                        self.status = Status::Failed;
                    }
                }
            }
        }

        self.status = Status::Finished;
    }
}

impl std::fmt::Debug for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Status::Pending => write!(f, "Pending"),
            Status::Running => write!(f, "Running"),
            Status::Finished => write!(f, "Finished"),
            Status::Failed => write!(f, "Failed"),
        }
    }
}

pub struct Task {
    pub steps: Vec<Step>,
    pub status: Status,
}

impl Clone for Task {
    fn clone(&self) -> Task {
        Task {
            steps: self.steps.clone(),
            status: self.status.clone(),
        }
    }
}

impl Task {
    pub async fn run(&mut self) {
        if self.status != Status::Pending {
            return;
        }

        self.status = Status::Running;

        for step in &mut self.steps {
            step.run().await;
        }
    }

    pub fn step(&mut self, spec: Spec) -> &mut Step {
        let step = Step {
            spec: spec,
            status: Status::Pending,
        };
        self.steps.push(step);
        self.steps.last_mut().unwrap()
    }
}
