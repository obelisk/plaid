#[macro_use]
extern crate log;

use performance::ModulePerformanceMetadata;
use plaid::{logging::Logger, *};

use apis::Api;
use data::Data;
use executor::*;
use storage::Storage;
use tokio::signal;

use std::sync::Arc;

use crossbeam_channel::bounded;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    info!("Plaid is booting up, please standby...");

    info!("Reading configuration");
    let mut configuration = config::configure()?;
    let (log_sender, log_receiver) = bounded(configuration.log_queue_size);

    info!("Starting logging subsystem");
    let (els, _logging_handler) = Logger::start(configuration.logging);
    info!("Logging subsystem started");

    // Create the storage system is one is configured
    let storage = match configuration.storage {
        Some(config) => Some(Arc::new(Storage::new(config)?)),
        None => None,
    };

    if storage.is_some() {
        info!("Storage system configured");
    } else {
        info!(
            "No persistent storage system configured; unexecuted log backs will be lost on shutdown"
        );
    }

    info!("Creating a Tokio runtime to run data fetching and ingestion...");
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    // This sender provides an internal route to sending logs. This is what
    // powers the logback functions.
    let delayed_log_sender = Data::start(
        configuration.data,
        log_sender.clone(),
        storage.clone(),
        els.clone(),
        runtime.handle().clone(),
    )
    .expect("The data system failed to start")
    .unwrap();

    info!("Configurating APIs for Modules");
    // Create the API that powers all the wrapped calls that modules can make
    let api = Api::new(
        configuration.apis,
        log_sender.clone(),
        delayed_log_sender,
        runtime.handle().clone(),
    )
    .unwrap();

    // Create an Arc so all the handlers have access to our API object
    let api = Arc::new(api);

    let performance_sender = if let Some(ref mut perf) = &mut configuration.performance_monitoring {
        warn!("Plaid is running with performance monitoring enabled - this is NOT recommended for production deployments. Metadata about rule execution will be logged to a channel that aggregates and reports metrics.");
        let (sender, rx) = crossbeam_channel::bounded::<ModulePerformanceMetadata>(4096);

        // Start the performance monitoring system which will handle
        // starting a thread to receive performance data
        perf.start(rx);

        Some(sender)
    } else {
        None
    };

    info!("Loading all the modules");
    // Load all the modules that form our Nanoservices and Plaid rules
    let modules = Arc::new(loader::load(configuration.loading).unwrap());
    let modules_by_name = Arc::new(modules.get_modules());

    info!(
        "Starting the execution threads of which {} were requested",
        configuration.execution_threads
    );

    // Create the executor that will handle all the logs that come in and immediate
    // requests for handling some configured get requests.
    let executor = Executor::new(
        log_receiver,
        modules.get_channels(),
        api,
        storage,
        configuration.execution_threads,
        els.clone(),
        performance_sender.clone(),
        modules_by_name,
    );

    let _executor = Arc::new(executor);

    info!("Starting servers, boot up complete");

    // let webhook_servers =
    //     data::webhook::configure_webhook_servers(config, log_sender, modules_by_name);
    //let mut join_set = JoinSet::from_iter(webhook_servers);

    // Graceful shutdown handling
    tokio::spawn(async move {
        signal::ctrl_c()
            .await
            .expect("Failed to listen for shutdown signal");
        tokio::runtime::Handle::current();
    });

    // Listen for a shutdown signal or if any task in join_set finishes
    // tokio::select! {
    //     _ = join_set.join_next() => {
    //         info!("A webserver task finished unexpectedly, triggering shutdown.");
    //         // Send a shutdown signal
    //         cancellation_token.cancel()
    //     },
    //     _ = cancellation_token.cancelled() => {
    //         info!("Shutdown signal received.");
    //     }
    // }

    // Ensure that the performance monitoring loop exits before finishing shutdown.
    // We do this to guarantee that rule performance data data gets written to a file.
    // if let Some(mut perf) = configuration.performance_monitoring {
    //     if let Some(handle) = perf.get_handle() {
    //         info!("Waiting for performance monitoring system to shutdown...");
    //         let metrics = handle
    //             .join()
    //             .expect("Performance monitoring system failed to shutdown");
    //         perf.generate_report(metrics).await?;
    //     } else {
    //         error!("Performance monitoring system failed to start");
    //     }
    // }

    // We can also trigger shutdown of the execution loop here and guarantee that no logs get dropped
    // on shutdown by waiting for the queue to empty.

    info!("Plaid shutdown complete.");
    Ok(())
}
