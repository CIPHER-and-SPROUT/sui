// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{HashMap, VecDeque},
    fs::{self},
    path::PathBuf,
    time::Duration,
};

use sui_config::genesis_config::GenesisConfig;
use tokio::time::{self, Instant};

use crate::{
    benchmark::{BenchmarkParameters, BenchmarkParametersGenerator},
    client::Instance,
    display, ensure,
    error::{TestbedError, TestbedResult},
    faults::CrashRecoverySchedule,
    logs::LogsAnalyzer,
    measurement::{Measurement, MeasurementsCollection},
    protocol::{sui::SuiProtocol, ProtocolCommands},
    settings::Settings,
    ssh::{CommandStatus, SshCommand, SshConnectionManager},
};

/// An orchestrator to run benchmarks on a testbed.
pub struct Orchestrator {
    /// The testbed's settings.
    settings: Settings,
    /// The state of the testbed (reflecting accurately the state of the machines).
    instances: Vec<Instance>,
    /// Provider-specific commands to install on the instance.
    instance_setup_commands: Vec<String>,
    /// Protocol-specific commands generator to generate the protocol configuration files,
    /// boot clients and nodes, etc.
    protocol_commands: SuiProtocol,
    /// The interval between measurements collection.
    scrape_interval: Duration,
    /// The interval to crash nodes.
    crash_interval: Duration,
    /// Handle ssh connections to instances.
    ssh_manager: SshConnectionManager,
    /// Whether to skip testbed updates before running benchmarks.
    skip_testbed_update: bool,
    /// Whether to skip testbed configuration before running benchmarks.
    skip_testbed_configuration: bool,
    /// Whether to downloading and analyze the client and node log files.
    log_processing: bool,
}

impl Orchestrator {
    /// The default interval between measurements collection.
    const DEFAULT_SCRAPE_INTERVAL: Duration = Duration::from_secs(15);
    /// The default interval to crash nodes.
    const DEFAULT_CRASH_INTERVAL: Duration = Duration::from_secs(60);

    /// Make a new orchestrator.
    pub fn new(
        settings: Settings,
        instances: Vec<Instance>,
        instance_setup_commands: Vec<String>,
        protocol_commands: SuiProtocol,
        ssh_manager: SshConnectionManager,
    ) -> Self {
        Self {
            settings,
            instances,
            instance_setup_commands,
            protocol_commands,
            ssh_manager,
            scrape_interval: Self::DEFAULT_SCRAPE_INTERVAL,
            crash_interval: Self::DEFAULT_CRASH_INTERVAL,
            skip_testbed_update: false,
            skip_testbed_configuration: false,
            log_processing: false,
        }
    }

    /// Set interval between measurements collection.
    pub fn with_scrape_interval(mut self, scrape_interval: Duration) -> Self {
        self.scrape_interval = scrape_interval;
        self
    }

    /// Set interval with which to crash nodes.
    pub fn with_crash_interval(mut self, crash_interval: Duration) -> Self {
        self.crash_interval = crash_interval;
        self
    }

    /// Whether to skip testbed updates before running benchmarks.
    pub fn skip_testbed_updates(mut self, skip_testbed_update: bool) -> Self {
        self.skip_testbed_update = skip_testbed_update;
        self
    }

    /// Whether to skip testbed configuration before running benchmarks.
    pub fn skip_testbed_configuration(mut self, skip_testbed_configuration: bool) -> Self {
        self.skip_testbed_configuration = skip_testbed_configuration;
        self
    }

    /// Whether to download and analyze the client and node log files.
    pub fn with_log_processing(mut self, log_processing: bool) -> Self {
        self.log_processing = log_processing;
        self
    }

    /// Select on which instances of the testbed to run the benchmarks.
    pub fn select_instances(
        &self,
        parameters: &BenchmarkParameters,
    ) -> TestbedResult<Vec<Instance>> {
        ensure!(
            self.instances.len() >= parameters.nodes,
            TestbedError::InsufficientCapacity(parameters.nodes - self.instances.len())
        );

        let mut instances_by_regions = HashMap::new();
        for instance in &self.instances {
            if instance.is_active() {
                instances_by_regions
                    .entry(&instance.region)
                    .or_insert_with(VecDeque::new)
                    .push_back(instance);
            }
        }

        let mut instances = Vec::new();
        for region in self.settings.regions.iter().cycle() {
            if instances.len() == parameters.nodes {
                break;
            }
            if let Some(regional_instances) = instances_by_regions.get_mut(region) {
                if let Some(instance) = regional_instances.pop_front() {
                    instances.push(instance.clone());
                }
            }
        }
        Ok(instances)
    }

    /// Boot one node per instance.
    async fn boot_nodes(&self, instances: Vec<Instance>) -> TestbedResult<()> {
        // Run one node per instance.
        let listen_addresses = SuiProtocol::make_listen_addresses(&instances);
        let working_dir = self.settings.working_dir.clone();
        let command = move |i: usize| -> String {
            let mut config_path = working_dir.clone();
            config_path.push("sui_config");
            config_path.push(format!("validator-config-{i}.yaml"));
            let address = listen_addresses[i].clone();
            let run = format!(
                "cargo run --release --bin sui-node -- --config-path {} --listen-address {address}",
                config_path.display()
            );
            ["source $HOME/.cargo/env", &run].join(" && ")
        };

        let repo = self.settings.repository_name();
        let ssh_command = SshCommand::new(command)
            .run_background("node".into())
            .with_log_file("~/node.log".into())
            .with_execute_from_path(repo.into());
        self.ssh_manager
            .execute(instances.iter(), &ssh_command)
            .await?;

        // Wait until all nodes are reachable.
        let metrics_command = format!("curl 127.0.0.1:{}/metrics", SuiProtocol::NODE_METRICS_PORT);
        let metrics_ssh_command = SshCommand::new(move |_| metrics_command.clone());
        self.ssh_manager
            .wait_for_success(instances.iter(), &metrics_ssh_command)
            .await;

        Ok(())
    }

    /// Install the codebase and its dependencies on the testbed.
    pub async fn install(&self) -> TestbedResult<()> {
        display::action("Installing dependencies on all machines");

        let working_dir = self.settings.working_dir.display();
        let url = &self.settings.repository.url;
        let basic_commands = [
            "sudo apt-get update",
            "sudo apt-get -y upgrade",
            "sudo apt-get -y autoremove",
            // Disable "pending kernel upgrade" message.
            "sudo apt-get -y remove needrestart",
            // The following dependencies prevent the error: [error: linker `cc` not found].
            "sudo apt-get -y install build-essential",
            // Install dependencies to compile 'plotter'.
            "sudo apt-get -y install libfontconfig libfontconfig1-dev",
            // Install prometheus.
            "sudo apt-get -y install prometheus",
            "sudo chmod 777 -R /var/lib/prometheus/",
            // Install rust (non-interactive).
            "curl --proto \"=https\" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
            "source $HOME/.cargo/env",
            "rustup default stable",
            // Create the working directory.
            &format!("mkdir -p {working_dir}"),
            // Clone the repo.
            &format!("(git clone {url} || true)"),
        ];

        let cloud_provider_specific_dependencies: Vec<_> = self
            .instance_setup_commands
            .iter()
            .map(|x| x.as_str())
            .collect();

        let protocol_dependencies = SuiProtocol::protocol_dependencies();

        let command = [
            &basic_commands[..],
            &cloud_provider_specific_dependencies[..],
            &protocol_dependencies[..],
        ]
        .concat()
        .join(" && ");

        let instances = self.instances.iter().filter(|x| x.is_active());
        let ssh_command = SshCommand::new(move |_| command.clone());
        self.ssh_manager.execute(instances, &ssh_command).await?;

        display::done();
        Ok(())
    }

    /// Update all instances to use the version of the codebase specified in the setting file.
    pub async fn update(&self) -> TestbedResult<()> {
        display::action("Updating all instances");

        // Update all active instances. This requires compiling the codebase in release (which
        // may take a long time) so we run the command in the background to avoid keeping alive
        // many ssh connections for too long.
        let commit = &self.settings.repository.commit;
        let command = [
            "git fetch -f",
            &format!("(git checkout -b {commit} {commit} || git checkout -f {commit})"),
            "(git pull -f || true)",
            "source $HOME/.cargo/env",
            "cargo build --release",
        ]
        .join(" && ");

        let instances = self.instances.iter().filter(|x| x.is_active());
        let id = "update";
        let repo_name = self.settings.repository_name();
        let ssh_command = SshCommand::new(move |_| command.clone())
            .run_background(id.into())
            .with_execute_from_path(repo_name.into());
        self.ssh_manager
            .execute(instances.clone(), &ssh_command)
            .await?;

        // Wait until the command finished running.
        self.ssh_manager
            .wait_for_command(instances, &ssh_command, CommandStatus::Terminated)
            .await?;

        display::done();
        Ok(())
    }

    /// Configure the instances with the appropriate configuration files.
    pub async fn configure(&self, parameters: &BenchmarkParameters) -> TestbedResult<()> {
        display::action("Configuring instances");

        // Select instances to configure.
        let instances = self.select_instances(parameters)?;

        // Generate the genesis configuration file and the keystore allowing access to gas objects.
        let command = self.protocol_commands.genesis_command(instances.iter());
        let repo_name = self.settings.repository_name();
        let ssh_command =
            SshCommand::new(move |_| command.clone()).with_execute_from_path(repo_name.into());
        self.ssh_manager
            .execute(instances.iter(), &ssh_command)
            .await?;

        display::done();
        Ok(())
    }

    /// Cleanup all instances and optionally delete their log files.
    pub async fn cleanup(&self, cleanup: bool) -> TestbedResult<()> {
        display::action("Cleaning up testbed");

        // Kill all tmux servers and delete the nodes dbs. Optionally clear logs.
        let mut command = vec!["(tmux kill-server || true)".into()];
        for path in self.protocol_commands.db_directories() {
            command.push(format!("(rm -rf {} || true)", path.display()));
        }
        if cleanup {
            command.push("(rm -rf ~/*log* || true)".into());
            // command.push("(rm -rf ~/*.yml || true)".into());
            // command.push("(rm -rf ~/*.keystore || true)".into());
        }
        let command = command.join(" ; ");

        // Execute the deletion on all machines.
        let instances = self.instances.iter().filter(|x| x.is_active());
        let ssh_command = SshCommand::new(move |_| command.clone());
        self.ssh_manager.execute(instances, &ssh_command).await?;

        display::done();
        Ok(())
    }

    /// Deploy the nodes. Optionally specify which instances to deploy; run the entire committee
    /// by default.
    pub async fn run_nodes(&self, parameters: &BenchmarkParameters) -> TestbedResult<()> {
        display::action("Deploying validators");

        // Select the instances to run.
        let instances = self.select_instances(parameters)?;

        // Boot one node per instance.
        self.boot_nodes(instances).await?;

        display::done();
        Ok(())
    }

    /// Deploy the load generators.
    pub async fn run_clients(&self, parameters: &BenchmarkParameters) -> TestbedResult<()> {
        display::action("Setting up load generators");

        // Select the instances to run.
        let instances = self.select_instances(parameters)?;

        // Deploy the load generators.
        let working_dir = self.settings.working_dir.clone();
        let committee_size = instances.len();
        let load_share = parameters.load / committee_size;
        let shared_counter = parameters.shared_objects_ratio;
        let transfer_objects = 100 - shared_counter;
        let command = move |i: usize| -> String {
            let mut genesis = working_dir.clone();
            genesis.push("sui_config");
            genesis.push("genesis.blob");
            let gas_id = GenesisConfig::benchmark_gas_object_id_offsets(committee_size)[i].clone();
            let keystore = format!(
                "~/working_dir/sui_config/{}",
                sui_config::SUI_BENCHMARK_GENESIS_GAS_KEYSTORE_FILENAME
            );

            let run = [
                "cargo run --release --bin stress --",
                "--local false --num-client-threads 100 --num-transfer-accounts 2 ",
                &format!(
                    "--genesis-blob-path {} --keystore-path {keystore}",
                    genesis.display()
                ),
                &format!("--primary-gas-id {}", gas_id),
                "bench",
                &format!("--num-workers 100 --in-flight-ratio 50 --target-qps {load_share}"),
                &format!("--shared-counter {shared_counter} --transfer-object {transfer_objects}"),
                &format!("--client-metric-port {}", SuiProtocol::CLIENT_METRICS_PORT),
            ]
            .join(" ");
            ["source $HOME/.cargo/env", &run].join(" && ")
        };

        let repo = self.settings.repository_name();
        let ssh_command = SshCommand::new(command)
            .run_background("client".into())
            .with_log_file("~/client.log".into())
            .with_execute_from_path(repo.into());
        self.ssh_manager
            .execute(instances.iter(), &ssh_command)
            .await?;

        // Wait until all load generators are reachable.
        let metrics_command = format!(
            "curl 127.0.0.1:{}/metrics",
            SuiProtocol::CLIENT_METRICS_PORT
        );
        let metrics_ssh_command = SshCommand::new(move |_| metrics_command.clone());
        self.ssh_manager
            .wait_for_success(instances.iter(), &metrics_ssh_command)
            .await;

        display::done();
        Ok(())
    }

    /// Collect metrics from the load generators.
    pub async fn run(
        &self,
        parameters: &BenchmarkParameters,
    ) -> TestbedResult<MeasurementsCollection> {
        display::action(format!(
            "Scraping metrics (at least {}s)",
            parameters.duration.as_secs()
        ));

        // Select the instances to run.
        let instances = self.select_instances(parameters)?;

        // Regularly scrape the client metrics.
        let command = format!(
            "curl 127.0.0.1:{}/metrics",
            SuiProtocol::CLIENT_METRICS_PORT
        );
        let ssh_command = SshCommand::new(move |_| command.clone());

        let mut aggregator = MeasurementsCollection::new(&self.settings, parameters.clone());
        let mut metrics_interval = time::interval(self.scrape_interval);
        metrics_interval.tick().await; // The first tick returns immediately.

        let faults_type = parameters.faults.clone();
        let mut faults_schedule = CrashRecoverySchedule::new(faults_type, instances.clone());
        let mut faults_interval = time::interval(self.crash_interval);
        faults_interval.tick().await; // The first tick returns immediately.

        let start = Instant::now();
        loop {
            tokio::select! {
                // Scrape metrics.
                now = metrics_interval.tick() => {
                    let elapsed = now.duration_since(start).as_secs_f64().ceil() as u64;
                    display::status(format!("{elapsed}s"));

                    let stdio = self
                        .ssh_manager
                        .execute(instances.iter(), &ssh_command)
                        .await?;
                    for (i, (stdout, _stderr)) in stdio.iter().enumerate() {
                        let measurement = Measurement::from_prometheus::<SuiProtocol>(stdout);
                        aggregator.add(i, measurement);
                    }

                    if aggregator.benchmark_duration() >= parameters.duration {
                        break;
                    } else if elapsed > (parameters.duration + self.scrape_interval).as_secs() {
                        display::error("Maximum scraping duration exceeded");
                        break;
                    }
                },

                // Kill and recover nodes according to the input schedule.
                _ = faults_interval.tick() => {
                    let action = faults_schedule.update();
                    if !action.kill.is_empty() {
                        self.ssh_manager.kill(action.kill.iter(), "node").await?;
                    }
                    if !action.boot.is_empty() {
                        self.boot_nodes(action.boot.clone()).await?;
                    }
                    if !action.kill.is_empty() || !action.boot.is_empty() {
                        display::newline();
                        display::config("Update testbed", action);
                    }
                }
            }
        }

        let results_directory = &self.settings.results_dir;
        let commit = &self.settings.repository.commit;
        let path: PathBuf = [results_directory, &format!("results-{commit}").into()]
            .iter()
            .collect();
        fs::create_dir_all(&path).expect("Failed to create log directory");
        aggregator.save(path);

        display::done();
        Ok(aggregator)
    }

    /// Download the log files from the nodes and clients.
    pub async fn download_logs(
        &self,
        parameters: &BenchmarkParameters,
    ) -> TestbedResult<LogsAnalyzer> {
        display::action("Downloading logs");
        // Select the instances to run.
        let instances = self.select_instances(parameters)?;

        // NOTE: Our ssh library does not seem to be able to transfers files in parallel reliably.
        let mut log_parsers = Vec::new();
        for (i, instance) in instances.iter().enumerate() {
            display::status(format!("{}/{}", i + 1, instances.len()));
            let mut log_parser = LogsAnalyzer::default();

            // Connect to the instance.
            let connection = self.ssh_manager.connect(instance.ssh_address()).await?;

            // Create a log sub-directory for this run.
            let commit = &self.settings.repository.commit;
            let path: PathBuf = [
                &self.settings.logs_dir,
                &format!("logs-{commit}").into(),
                &format!("logs-{parameters:?}").into(),
            ]
            .iter()
            .collect();
            fs::create_dir_all(&path).expect("Failed to create log directory");

            // Download the node log files.
            let node_log_content = connection.download("node.log")?;
            log_parser.set_node_errors(&node_log_content);

            let node_log_file = [path.clone(), format!("node-{i}.log").into()]
                .iter()
                .collect::<PathBuf>();
            fs::write(&node_log_file, node_log_content.as_bytes()).expect("Cannot write log file");

            // Download the clients log files.
            let client_log_content = connection.download("client.log")?;
            log_parser.set_client_errors(&client_log_content);

            let client_log_file = [path, format!("client-{i}.log").into()]
                .iter()
                .collect::<PathBuf>();
            fs::write(&client_log_file, client_log_content.as_bytes())
                .expect("Cannot write log file");

            log_parsers.push(log_parser)
        }

        display::done();
        Ok(LogsAnalyzer::aggregate(log_parsers))
    }

    /// Run all the benchmarks specified by the benchmark generator.
    pub async fn run_benchmarks(
        &mut self,
        mut generator: BenchmarkParametersGenerator,
    ) -> TestbedResult<()> {
        display::header("Preparing testbed");
        display::config("Commit", format!("'{}'", &self.settings.repository.commit));
        display::newline();

        // Cleanup the testbed (in case the previous run was not completed).
        self.cleanup(true).await?;

        // Update the software on all instances.
        if !self.skip_testbed_update {
            self.install().await?;
            self.update().await?;
        }

        // Run all benchmarks.
        let mut i = 1;
        let mut latest_committee_size = 0;
        while let Some(parameters) = generator.next() {
            display::header(format!("Starting benchmark {i}"));
            let ratio = &parameters.shared_objects_ratio;
            display::config("Load type", format!("{ratio}% shared objects"));
            display::config("Parameters", &parameters);
            display::newline();

            // Cleanup the testbed (in case the previous run was not completed).
            self.cleanup(true).await?;

            // Configure all instances (if needed).
            if !self.skip_testbed_configuration && latest_committee_size != parameters.nodes {
                self.configure(&parameters).await?;
                latest_committee_size = parameters.nodes;
            }

            // Deploy the validators.
            self.run_nodes(&parameters).await?;

            // Deploy the load generators.
            self.run_clients(&parameters).await?;

            // Wait for the benchmark to terminate. Then save the results and print a summary.
            let aggregator = self.run(&parameters).await?;
            aggregator.display_summary();
            generator.register_result(aggregator);

            // Kill the nodes and clients (without deleting the log files).
            self.cleanup(false).await?;

            // Download the log files.
            if self.log_processing {
                let error_counter = self.download_logs(&parameters).await?;
                error_counter.print_summary();
            }

            i += 1;
        }

        display::header("Benchmark completed");
        Ok(())
    }
}
