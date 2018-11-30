use crate::types::{Call, Fallible, Id, Message, ToInt, ToParams};
use crate::vim::RawMessage;
use failure::{bail, format_err};
use futures::sync::{mpsc as fmpsc, oneshot};
use jsonrpc_core as rpc;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::str::FromStr;
use std::sync::{mpsc, Arc, Mutex};
use tokio::await;

type Callback = (Id, oneshot::Sender<rpc::Output>);

/// JSONRPC client.
#[derive(Serialize)]
pub struct RpcClient {
    pub languageId: Option<String>,
    /// Incremental message id.
    id: Id,
    #[serde(skip_serializing)]
    /// Writer to server.
    writer: Arc<Mutex<Write>>,
    #[serde(skip_serializing)]
    /// Send requests "callbacks" into loop.
    tx: mpsc::Sender<Callback>,
}

impl RpcClient {
    pub fn new(
        reader: impl BufRead + Send + 'static,
        writer: impl Write + Send + 'static,
        sink: fmpsc::UnboundedSender<Call>,
        languageId: Option<String>,
    ) -> Fallible<RpcClient> {
        let (tx, rx): (mpsc::Sender<Callback>, mpsc::Receiver<Callback>) = mpsc::channel();
        let languageId_clone = languageId.clone();

        std::thread::Builder::new()
            .name(format!(
                "reader-{}",
                languageId.clone().unwrap_or_else(|| "main".into())
            ))
            .spawn(move || {
                let loop_read = move || {
                    let languageId = languageId_clone;
                    // Count how many consequent empty lines.
                    let mut count_empty_lines = 0;
                    let mut pending_requests = HashMap::new();

                    let mut reader = reader;
                    let mut content_length = 0;
                    loop {
                        match rx.try_recv() {
                            Ok((id, tx)) => {
                                pending_requests.insert(id, tx);
                            }
                            Err(mpsc::TryRecvError::Disconnected) => bail!("Disconnected!"),
                            Err(mpsc::TryRecvError::Empty) => (),
                        };

                        let mut message = String::new();
                        let mut line = String::new();
                        if languageId.is_some() {
                            reader.read_line(&mut line)?;
                            let line = line.trim();
                            if line.is_empty() {
                                count_empty_lines += 1;
                                if count_empty_lines > 5 {
                                    bail!("Unable to read from language server");
                                }

                                let mut buf = vec![0; content_length];
                                reader.read_exact(buf.as_mut_slice())?;
                                message = String::from_utf8(buf)?;
                            } else {
                                count_empty_lines = 0;
                                if !line.starts_with("Content-Length") {
                                    continue;
                                }

                                let tokens: Vec<&str> = line.splitn(2, ':').collect();
                                let len = tokens
                                    .get(1)
                                    .ok_or_else(|| {
                                        format_err!("Failed to get length! tokens: {:?}", tokens)
                                    })?
                                    .trim();
                                content_length = usize::from_str(len)?;
                            }
                        } else if reader.read_line(&mut message)? == 0 {
                            break;
                        }

                        let message = message.trim();
                        if message.is_empty() {
                            continue;
                        }
                        info!("<= {:?} {}", languageId, message);
                        // FIXME: Remove extra `meta` property from javascript-typescript-langserver.
                        let s = message.replace(r#","meta":{}"#, "");
                        let message = serde_json::from_str(&s);
                        if let Err(ref err) = message {
                            error!(
                                "Failed to deserialize output: {}\n\n Message: {}\n\nError: {:?}",
                                err, s, err
                            );
                            continue;
                        }
                        // TODO: cleanup.
                        let message = message.unwrap();
                        match message {
                            RawMessage::MethodCall(method_call) => {
                                sink.unbounded_send(Call::MethodCall(
                                    languageId.clone(),
                                    method_call,
                                ))?;
                            }
                            RawMessage::Notification(notification) => {
                                sink.unbounded_send(Call::Notification(
                                    languageId.clone(),
                                    notification,
                                ))?;
                            }
                            RawMessage::Output(output) => {
                                let id = output.id().to_int()?;
                                pending_requests
                                    .remove(&id)
                                    .ok_or_else(|| {
                                        format_err!("Pending request with id ({}) not found!", id)
                                    })?
                                    .send(output)
                                    .map_err(|output| {
                                        format_err!("Failed to send output: {:?}", output)
                                    })?;
                            }
                        };
                    }

                    Ok(())
                };

                if let Err(err) = loop_read() {
                    error!("{:?}", err);
                }
            })?;

        Ok(RpcClient {
            languageId,
            id: 0,
            writer: Arc::new(Mutex::new(writer)),
            tx,
        })
    }

    fn write(&mut self, message: impl AsRef<str>) -> Fallible<()> {
        let message = message.as_ref();
        info!("=> {:?} {}", self.languageId, message);
        write!(
            self.writer.lock().unwrap(),
            "Content-Length: {}\r\n\r\n{}",
            message.len(),
            message
        )?;
        self.writer.lock().unwrap().flush()?;
        Ok(())
    }

    pub async fn call<R>(
        &mut self,
        method: impl AsRef<str> + 'static,
        params: impl Serialize + 'static,
    ) -> Fallible<R>
    where
        R: DeserializeOwned,
    {
        let method = method.as_ref();
        self.id += 1;

        let method_call = rpc::MethodCall {
            jsonrpc: Some(rpc::Version::V2),
            id: rpc::Id::Num(self.id),
            method: method.into(),
            params: params.to_params()?,
        };

        let message = serde_json::to_string(&method_call)?;
        self.write(&message)?;

        let (tx, rx) = oneshot::channel();
        self.tx.send((self.id, tx))?;
        let result = await!(rx)?;

        match result {
            rpc::Output::Success(s) => Ok(serde_json::from_value(s.result)?),
            rpc::Output::Failure(f) => {
                // TODO
                Err(format_err!("{}", f.error.message))
            }
        }
    }

    pub fn notify(&mut self, method: impl AsRef<str>, params: impl Serialize) -> Fallible<()> {
        let method = method.as_ref();

        let notification = rpc::Notification {
            jsonrpc: Some(rpc::Version::V2),
            method: method.to_owned(),
            params: params.to_params()?,
        };

        let message = serde_json::to_string(&notification)?;
        self.write(&message)
    }
}
