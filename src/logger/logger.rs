use super::blocking;
#[cfg(feature = "nonblocking")]
use super::nonblocking;
use super::{level::DataDogLogLevel, log::DataDogLog};
use crate::{
    client::{AsyncDataDogClient, DataDogClient},
    config::DataDogConfig,
};
use flume::{bounded, unbounded, Receiver, Sender};
#[cfg(feature = "nonblocking")]
use futures::Future;
use std::{fmt::Display, ops::Drop, thread};

#[derive(Debug)]
/// Logger that logs directly to DataDog via HTTP(S)
pub struct DataDogLogger {
    config: DataDogConfig,
    logsender: Option<Sender<DataDogLog>>,
    selflogrv: Option<Receiver<String>>,
    selflogsd: Option<Sender<String>>,
    logger_handle: Option<thread::JoinHandle<()>>,
}

impl DataDogLogger {
    /// Exposes self log of the logger.
    ///
    /// Contains diagnostic messages with details of errors occuring inside logger.
    /// It will be `None`, unless `enable_self_log` in [`DataDogConfig`](crate::config::DataDogConfig) is set to `true`.
    pub fn selflog(&self) -> &Option<Receiver<String>> {
        &self.selflogrv
    }

    /// Creates new blocking DataDogLogger instance
    ///
    /// What it means is that no executor is used to host DataDog network client. A new thread is started instead.
    /// It receives messages to log and sends them in batches in blocking fashion.
    /// As this is a separate thread, calling [`log`](Self::log) does not imply any IO operation, thus is quite fast.
    pub fn blocking<T>(client: T, config: DataDogConfig) -> Self
    where
        T: DataDogClient + Send + 'static,
    {
        let (slsender, slreceiver) = if config.enable_self_log {
            let (s, r) = bounded::<String>(100);
            (Some(s), Some(r))
        } else {
            (None, None)
        };
        let slogsender_clone = slsender.clone();
        let (sender, receiver) = match config.messages_channel_capacity {
            Some(capacity) => bounded(capacity),
            None => unbounded(),
        };

        let logger_handle =
            thread::spawn(move || blocking::logger_thread(client, receiver, slsender));

        DataDogLogger {
            config,
            logsender: Some(sender),
            selflogrv: slreceiver,
            selflogsd: slogsender_clone,
            logger_handle: Some(logger_handle),
        }
    }

    /// Creates new non-blocking `DataDogLogger` instance
    ///
    /// Internally spawns logger future to `tokio` runtime.
    ///
    /// It is equivalent to calling [`non_blocking_cold`](Self::non_blocking_cold) and spawning future to Tokio runtime.
    /// Thus it is only a convinience function.
    #[cfg(feature = "with-tokio")]
    pub fn non_blocking_with_tokio<T>(client: T, config: DataDogConfig) -> Self
    where
        T: AsyncDataDogClient + Send + 'static,
    {
        let (logger, future) = Self::non_blocking_cold(client, config);
        tokio::spawn(future);
        logger
    }

    /// Creates new non-blocking `DataDogLogger` instance
    ///
    /// What it means is that logger requires executor to run. This executor will host a task that will receive messages to log.
    /// It will log them using non blocking (asynchronous) implementation of network client.
    ///
    /// It returns a `Future` that needs to be spawned for logger to work. This `Future` is a task that is responsible for sending messages.
    /// Although a little inconvinient, it is completely executor agnostic.
    #[cfg(feature = "nonblocking")]
    pub fn non_blocking_cold<T>(
        client: T,
        config: DataDogConfig,
    ) -> (Self, impl Future<Output = ()>)
    where
        T: AsyncDataDogClient,
    {
        let (slsender, slreceiver) = if config.enable_self_log {
            let (s, r) = bounded::<String>(100);
            (Some(s), Some(r))
        } else {
            (None, None)
        };
        let slogsender_clone = slsender.clone();
        let (logsender, logreceiver) = match config.messages_channel_capacity {
            Some(capacity) => bounded(capacity),
            None => unbounded(),
        };
        let logger_future = nonblocking::logger_future(client, logreceiver, slsender);

        let logger = DataDogLogger {
            config,
            logsender: Some(logsender),
            selflogrv: slreceiver,
            selflogsd: slogsender_clone,
            logger_handle: None,
        };

        (logger, logger_future)
    }

    /// Sends log to DataDog thread or task.
    ///
    /// This function does not invoke any IO operation by itself. Instead it sends messages to logger thread or task using channels.
    /// Therefore it is quite lightweight.
    pub fn log<T: Display>(&self, message: T, level: DataDogLogLevel) {
        let log = DataDogLog {
            message: message.to_string(),
            ddtags: self.config.tags.clone(),
            service: self.config.service.clone().unwrap_or_default(),
            host: self.config.hostname.clone().unwrap_or_default(),
            ddsource: self.config.source.clone(),
            level: level.to_string(),
        };

        if let Some(ref sender) = self.logsender {
            match sender.try_send(log) {
                Ok(()) => {
                    // nothing
                }
                Err(e) => {
                    if let Some(ref selflog) = self.selflogsd {
                        selflog.try_send(e.to_string()).unwrap_or_default();
                    }
                }
            }
        }
    }
}

impl Drop for DataDogLogger {
    fn drop(&mut self) {
        // drop sender to allow logger thread to close
        std::mem::drop(self.logsender.take());

        // wait for logger thread to finish to ensure all messages are flushed
        if let Some(handle) = self.logger_handle.take() {
            handle.join().unwrap_or_default();
        }
    }
}
