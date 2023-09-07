mod config;

use anyhow::Result;
use docker_api::conn::TtyChunk;
use docker_api::opts::{
    ContainerCreateOpts, ContainerListOpts, ContainerStopOpts, HostPort, LogsOpts, PublishPort,
};
use docker_api::{ApiVersion, Container, Docker};
use futures::stream::StreamExt;
use rosetta_client::{Signer, Wallet};
use rosetta_core::{BlockchainClient, BlockchainConfig};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio_retry::{strategy::ExponentialBackoff, RetryIf};

pub struct Env<T> {
    client: Arc<T>,
    node: Container,
}

impl<T: BlockchainClient> Env<T> {
    pub async fn new<Fut, F>(
        prefix: &str,
        mut config: BlockchainConfig,
        start_connector: F,
    ) -> Result<Env<T>>
    where
        Fut: Future<Output = Result<T>> + Send,
        F: FnMut(BlockchainConfig) -> Fut,
    {
        env_logger::try_init().ok();
        let builder = EnvBuilder::new(prefix)?;
        let node_port = builder.random_port();
        config.node_uri.port = node_port;
        log::info!("node: {}", node_port);
        builder.stop_container(&builder.node_name(&config)).await?;
        let node = builder.run_node(&config).await?;

        let client = match builder
            .run_connector::<T, Fut, F>(start_connector, config)
            .await
        {
            Ok(connector) => connector,
            Err(e) => {
                let opts = ContainerStopOpts::builder().build();
                let _ = node.stop(&opts).await;
                return Err(e);
            }
        };

        Ok(Self {
            client: Arc::new(client),
            node,
        })
    }

    pub fn node(&self) -> Arc<T> {
        Arc::clone(&self.client)
    }

    pub fn ephemeral_wallet(&self) -> Result<Wallet<Arc<T>>> {
        let signer = Signer::generate()?;
        Wallet::new(self.client.clone(), &signer)
    }

    pub async fn shutdown(self) -> Result<()> {
        let opts = ContainerStopOpts::builder().build();
        self.node.stop(&opts).await?;
        Ok(())
    }
}

struct EnvBuilder<'a> {
    prefix: &'a str,
    docker: Docker,
}

impl<'a> EnvBuilder<'a> {
    pub fn new(prefix: &'a str) -> Result<Self> {
        let version = ApiVersion::new(1, Some(41), None);
        let endpoint = config::docker_endpoint();
        let docker = Docker::new_versioned(endpoint, version)?;
        Ok(Self { prefix, docker })
    }

    fn random_port(&self) -> u16 {
        let mut bytes = [0; 2];
        getrandom::getrandom(&mut bytes).unwrap();
        u16::from_le_bytes(bytes)
    }

    fn node_name(&self, config: &BlockchainConfig) -> String {
        format!(
            "{}-node-{}-{}",
            self.prefix, config.blockchain, config.network
        )
    }

    async fn stop_container(&self, name: &str) -> Result<()> {
        let opts = ContainerListOpts::builder().all(true).build();
        for container in self.docker.containers().list(&opts).await? {
            if container
                .names
                .as_ref()
                .unwrap()
                .iter()
                .any(|n| n.as_str().ends_with(name))
            {
                let container = Container::new(self.docker.clone(), container.id.unwrap());
                log::info!("stopping {}", name);
                container
                    .stop(&ContainerStopOpts::builder().build())
                    .await?;
                container.delete().await.ok();
                break;
            }
        }
        Ok(())
    }

    async fn run_container(&self, name: String, opts: &ContainerCreateOpts) -> Result<Container> {
        log::info!("creating {}", name);
        let id = self.docker.containers().create(opts).await?.id().clone();
        let container = Container::new(self.docker.clone(), id.clone());
        container.start().await?;

        log::info!("starting {}", name);
        let container = Container::new(self.docker.clone(), id.clone());
        tokio::task::spawn(async move {
            let opts = LogsOpts::builder()
                .all()
                .follow(true)
                .stdout(true)
                .stderr(true)
                .build();
            let mut logs = container.logs(&opts);
            while let Some(chunk) = logs.next().await {
                match chunk {
                    Ok(TtyChunk::StdOut(stdout)) => {
                        let stdout = std::str::from_utf8(&stdout).unwrap_or_default();
                        log::info!("{}: stdout: {}", name, stdout);
                    }
                    Ok(TtyChunk::StdErr(stderr)) => {
                        let stderr = std::str::from_utf8(&stderr).unwrap_or_default();
                        log::info!("{}: stderr: {}", name, stderr);
                    }
                    Err(err) => {
                        log::error!("{}", err);
                    }
                    Ok(TtyChunk::StdIn(_)) => unreachable!(),
                }
            }
            log::info!("{}: exited", name);
        });

        let container = Container::new(self.docker.clone(), id.clone());
        loop {
            match health(&container).await? {
                Some(Health::Unhealthy) => anyhow::bail!("healthcheck reports unhealthy"),
                Some(Health::Starting) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                _ => break,
            }
        }

        Ok(container)
    }

    async fn run_node(&self, config: &BlockchainConfig) -> Result<Container> {
        let name = self.node_name(config);
        let mut opts = ContainerCreateOpts::builder()
            .name(&name)
            .image(config.node_image)
            .command((config.node_command)(config.network, config.node_uri.port))
            .auto_remove(true)
            .attach_stdout(true)
            .attach_stderr(true)
            .publish(PublishPort::tcp(config.node_uri.port as _))
            .expose(
                PublishPort::tcp(config.node_uri.port as _),
                HostPort::new(config.node_uri.port as u32),
            );
        for port in config.node_additional_ports {
            let port = *port as u32;
            opts = opts.expose(PublishPort::tcp(port), port);
        }
        let container = self.run_container(name, &opts.build()).await?;

        // TODO: replace this by a proper healthcheck
        let maybe_error = if matches!(config.node_uri.scheme, "http" | "https" | "ws" | "wss") {
            wait_for_http(
                config
                    .node_uri
                    .with_scheme("http") // any ws endpoint is also a http endpoint
                    .with_host("127.0.0.1")
                    .to_string(),
                &container,
            )
            .await
            .err()
        } else {
            // Wait 15 seconds to guarantee the node didn't crash
            tokio::time::sleep(Duration::from_secs(15)).await;
            health(&container).await.err()
        };

        if let Some(err) = maybe_error {
            log::error!("node failed to start: {}", err);
            let _ = container.stop(&ContainerStopOpts::default()).await;
            return Err(err);
        }
        Ok(container)
    }

    async fn run_connector<T, Fut, F>(
        &self,
        mut start_connector: F,
        config: BlockchainConfig,
    ) -> Result<T>
    where
        T: BlockchainClient,
        Fut: Future<Output = Result<T>> + Send,
        F: FnMut(BlockchainConfig) -> Fut,
    {
        const MAX_RETRIES: usize = 10;

        let client = {
            let retry_strategy = tokio_retry::strategy::FibonacciBackoff::from_millis(1000)
                .max_delay(Duration::from_secs(5))
                .take(MAX_RETRIES);
            let mut result = Err(anyhow::anyhow!("failed to start connector"));
            for delay in retry_strategy {
                match start_connector(config.clone()).await {
                    Ok(client) => {
                        result = Ok(client);
                        break;
                    }
                    Err(error) => {
                        result = Err(error);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                }
            }
            result?
        };

        Ok(client)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Health {
    None,
    Starting,
    Healthy,
    Unhealthy,
}

async fn health(container: &Container) -> Result<Option<Health>> {
    let inspect = container.inspect().await?;
    let status = inspect
        .state
        .and_then(|state| state.health)
        .and_then(|health| health.status);
    let Some(status) = status else {
        return Ok(None);
    };
    Ok(Some(match status.as_str() {
        "none" => Health::None,
        "starting" => Health::Starting,
        "healthy" => Health::Healthy,
        "unhealthy" => Health::Unhealthy,
        status => anyhow::bail!("unknown status {}", status),
    }))
}

async fn wait_for_http<S: AsRef<str>>(url: S, container: &Container) -> Result<()> {
    let url = url.as_ref();
    let retry_strategy = ExponentialBackoff::from_millis(2)
        .factor(100)
        .max_delay(Duration::from_secs(2))
        .take(20); // limit to 20 retries

    #[derive(Debug)]
    enum RetryError {
        Retry(anyhow::Error),
        ContainerExited(anyhow::Error),
    }

    RetryIf::spawn(
        retry_strategy,
        || async move {
            match surf::get(url).await {
                Ok(_) => Ok(()),
                Err(err) => {
                    // Check if the container exited
                    let health_status = health(container).await;
                    if matches!(health_status, Err(_) | Ok(Some(Health::Unhealthy))) {
                        return Err(RetryError::ContainerExited(err.into_inner()));
                    }
                    Err(RetryError::Retry(err.into_inner()))
                }
            }
        },
        // Retry Condition
        |error: &RetryError| matches!(error, RetryError::Retry(_)),
    )
    .await
    .map_err(|err| match err {
        RetryError::Retry(error) => error,
        RetryError::ContainerExited(error) => error,
    })
}
