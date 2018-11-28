use crate::types::{Call, Fallible, Id, Message};
use crate::vim::RawMessage;
use failure::{bail, format_err};
use futures::sync::{mpsc as fmpsc, oneshot};
use jsonrpc_core as rpc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::str::FromStr;
use std::sync::mpsc;

/// JSONRPC client.
pub struct RpcClient<W>
where
    W: Write,
{
    /// Output. Writer to server.
    output: BufWriter<W>,
    /// Send requests "callbacks" into loop.
    tx: mpsc::Sender<(Id, oneshot::Sender<Value>)>,
}

impl<W> RpcClient<W>
where
    W: Write,
{
    pub fn new<R>(
        input: R,
        output: BufWriter<W>,
        sink: mpsc::Sender<Call>,
        languageId: Option<String>,
    ) -> Fallible<RpcClient<W>>
    where
        R: 'static + BufRead + Send,
    {
        let (tx, rx) = mpsc::channel();

        std::thread::Builder::new()
            .name(format!(
                "reader-{}",
                languageId.clone().unwrap_or_else(|| "main".into())
            ))
            .spawn(move || {
                let loop_read = move || {
                    // Count how many consequent empty lines.
                    let mut count_empty_lines = 0;

                    let mut input = input;
                    let mut content_length = 0;
                    loop {
                        let mut message = String::new();
                        let mut line = String::new();
                        if languageId.is_some() {
                            input.read_line(&mut line)?;
                            let line = line.trim();
                            if line.is_empty() {
                                count_empty_lines += 1;
                                if count_empty_lines > 5 {
                                    bail!("Unable to read from language server");
                                }

                                let mut buf = vec![0; content_length];
                                input.read_exact(buf.as_mut_slice())?;
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
                        } else if input.read_line(&mut message)? == 0 {
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
                        let message = match message {
                            RawMessage::MethodCall(method_call) => {
                                Message::MethodCall(languageId.clone(), method_call)
                            }
                            RawMessage::Notification(notification) => {
                                Message::Notification(languageId.clone(), notification)
                            }
                            RawMessage::Output(output) => Message::Output(output),
                        };

                        // TODO
                    }

                    Ok(())
                };

                if let Err(err) = loop_read() {
                    error!("{:?}", err);
                }
            })?;

        Ok(RpcClient { output, tx })
    }

    pub async fn call(&self) -> Fallible<Value> {
        Ok(json!({}))
    }

    pub fn notify(&self) -> Fallible<()> {
        Ok(())
    }
}
