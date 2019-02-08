use super::*;
use crate::context::Context;
use crate::language_client::LanguageClient;
use crate::lsp::notification::Notification;
use crate::lsp::request::Request;

impl LanguageClient {
    pub fn handle_call(&self, msg: Call) -> Fallible<()> {
        match msg {
            Call::MethodCall(lang_id, method_call) => {
                let result = self.handle_method_call(lang_id.as_deref(), &method_call);
                if let Err(ref err) = result {
                    if err.find_root_cause().downcast_ref::<LCError>().is_none() {
                        error!(
                            "Error handling message: {}\n\nMessage: {}\n\nError: {:?}",
                            err,
                            serde_json::to_string(&method_call).unwrap_or_default(),
                            err
                        );
                    }
                }
                self.get_client(&lang_id)?
                    .output(method_call.id.to_int()?, result)?;
            }
            Call::Notification(lang_id, notification) => {
                let result = self.handle_notification(lang_id.as_deref(), &notification);
                if let Err(ref err) = result {
                    if err.downcast_ref::<LCError>().is_none() {
                        error!(
                            "Error handling message: {}\n\nMessage: {}\n\nError: {:?}",
                            err,
                            serde_json::to_string(&notification).unwrap_or_default(),
                            err
                        );
                    }
                }
            }
        }

        // TODO
        if let Err(err) = self.handle_fs_events() {
            warn!("{:?}", err);
        }

        Ok(())
    }

    pub fn handle_method_call(
        &self,
        languageId: Option<&str>,
        method_call: &rpc::MethodCall,
    ) -> Fallible<Value> {
        let params = serde_json::to_value(method_call.params.clone())?;

        let user_handler =
            self.get(|state| state.user_handlers.get(&method_call.method).cloned())?;
        if let Some(user_handler) = user_handler {
            return self.vim()?.rpcclient.call(&user_handler, params);
        }

        let ctx = Context::new(self.vim()?, &method_call.params);

        match method_call.method.as_str() {
            lsp::request::RegisterCapability::METHOD => {
                self.client_registerCapability(languageId.unwrap_or_default(), &params, &ctx)
            }
            lsp::request::UnregisterCapability::METHOD => {
                self.client_unregisterCapability(languageId.unwrap_or_default(), &params, &ctx)
            }
            lsp::request::HoverRequest::METHOD => self.textDocument_hover(&params, &ctx),
            REQUEST__FindLocations => self.find_locations(&params, &ctx),
            lsp::request::Rename::METHOD => self.textDocument_rename(&params, &ctx),
            lsp::request::DocumentSymbolRequest::METHOD => {
                self.textDocument_documentSymbol(&params, &ctx)
            }
            lsp::request::WorkspaceSymbol::METHOD => self.workspace_symbol(&params, &ctx),
            lsp::request::CodeActionRequest::METHOD => self.textDocument_codeAction(&params, &ctx),
            lsp::request::Completion::METHOD => self.textDocument_completion(&params, &ctx),
            lsp::request::SignatureHelpRequest::METHOD => {
                self.textDocument_signatureHelp(&params, &ctx)
            }
            lsp::request::References::METHOD => self.textDocument_references(&params, &ctx),
            lsp::request::Formatting::METHOD => self.textDocument_formatting(&params, &ctx),
            lsp::request::RangeFormatting::METHOD => {
                self.textDocument_rangeFormatting(&params, &ctx)
            }
            lsp::request::ResolveCompletionItem::METHOD => {
                self.completionItem_resolve(&params, &ctx)
            }
            lsp::request::ExecuteCommand::METHOD => self.workspace_executeCommand(&params, &ctx),
            lsp::request::ApplyWorkspaceEdit::METHOD => self.workspace_applyEdit(&params, &ctx),
            lsp::request::DocumentHighlightRequest::METHOD => {
                self.textDocument_documentHighlight(&params, &ctx)
            }
            // Extensions.
            REQUEST__GetState => self.languageClient_getState(&params, &ctx),
            REQUEST__IsAlive => self.languageClient_isAlive(&params, &ctx),
            REQUEST__StartServer => self.languageClient_startServer(&params, &ctx),
            REQUEST__RegisterServerCommands => {
                self.languageClient_registerServerCommands(&params, &ctx)
            }
            REQUEST__SetLoggingLevel => self.languageClient_setLoggingLevel(&params, &ctx),
            REQUEST__SetDiagnosticsList => self.languageClient_setDiagnosticsList(&params, &ctx),
            REQUEST__RegisterHandlers => self.languageClient_registerHandlers(&params, &ctx),
            REQUEST__NCMRefresh => self.NCM_refresh(&params),
            REQUEST__NCM2OnComplete => self.NCM2_on_complete(&params),
            REQUEST__ExplainErrorAtPoint => self.languageClient_explainErrorAtPoint(&params, &ctx),
            REQUEST__OmniComplete => self.languageClient_omniComplete(&params, &ctx),
            REQUEST__ClassFileContents => self.java_classFileContents(&params, &ctx),
            REQUEST__DebugInfo => self.debug_info(&params, &ctx),

            _ => {
                let languageId_target = if languageId.is_some() {
                    // Message from language server. No handler found.
                    let msg = format!("Message not handled: {:?}", method_call);
                    if method_call.method.starts_with('$') {
                        warn!("{}", msg);
                        return Ok(Value::default());
                    } else {
                        return Err(err_msg(msg));
                    }
                } else {
                    // Message from vim. Proxy to language server.
                    let (languageId_target,): (String,) =
                        self.gather_args(&[VimVar::LanguageId], &params)?;
                    info!(
                        "Proxy message directly to language server: {:?}",
                        method_call
                    );
                    Some(languageId_target)
                };

                self.get_client(&languageId_target)?
                    .call(&method_call.method, &params)
            }
        }
    }

    pub fn handle_notification(
        &self,
        languageId: Option<&str>,
        notification: &rpc::Notification,
    ) -> Fallible<()> {
        let params = serde_json::to_value(notification.params.clone())?;

        let user_handler =
            self.get(|state| state.user_handlers.get(&notification.method).cloned())?;
        if let Some(user_handler) = user_handler {
            return self.vim()?.rpcclient.notify(&user_handler, params.clone());
        }

        let ctx = Context::new(self.vim()?, &notification.params);

        match notification.method.as_str() {
            lsp::notification::DidChangeConfiguration::METHOD => {
                self.workspace_didChangeConfiguration(&params, &ctx)?
            }
            lsp::notification::DidOpenTextDocument::METHOD => {
                self.textDocument_didOpen(&params, &ctx)?
            }
            lsp::notification::DidChangeTextDocument::METHOD => {
                self.textDocument_didChange(&params, &ctx)?
            }
            lsp::notification::DidSaveTextDocument::METHOD => {
                self.textDocument_didSave(&params, &ctx)?
            }
            lsp::notification::DidCloseTextDocument::METHOD => {
                self.textDocument_didClose(&params, &ctx)?
            }
            lsp::notification::PublishDiagnostics::METHOD => {
                self.textDocument_publishDiagnostics(&params, &ctx)?
            }
            lsp::notification::LogMessage::METHOD => self.window_logMessage(&params, &ctx)?,
            lsp::notification::ShowMessage::METHOD => self.window_showMessage(&params, &ctx)?,
            lsp::notification::Exit::METHOD => self.exit(&params, &ctx)?,
            // Extensions.
            NOTIFICATION__HandleFileType => self.languageClient_handleFileType(&params, &ctx)?,
            NOTIFICATION__HandleBufNewFile => {
                self.languageClient_handleBufNewFile(&params, &ctx)?
            }
            NOTIFICATION__HandleTextChanged => {
                self.languageClient_handleTextChanged(&params, &ctx)?
            }
            NOTIFICATION__HandleBufWritePost => {
                self.languageClient_handleBufWritePost(&params, &ctx)?
            }
            NOTIFICATION__HandleBufDelete => self.languageClient_handleBufDelete(&params, &ctx)?,
            NOTIFICATION__HandleCursorMoved => {
                self.languageClient_handleCursorMoved(&params, &ctx)?
            }
            NOTIFICATION__HandleCompleteDone => {
                self.languageClient_handleCompleteDone(&params, &ctx)?
            }
            NOTIFICATION__FZFSinkLocation => self.languageClient_FZFSinkLocation(&params, &ctx)?,
            NOTIFICATION__FZFSinkCommand => self.languageClient_FZFSinkCommand(&params, &ctx)?,
            NOTIFICATION__ClearDocumentHighlight => {
                self.languageClient_clearDocumentHighlight(&params, &ctx)?
            }
            // Extensions by language servers.
            NOTIFICATION__LanguageStatus => self.language_status(&params, &ctx)?,
            NOTIFICATION__RustBeginBuild => self.rust_handleBeginBuild(&params, &ctx)?,
            NOTIFICATION__RustDiagnosticsBegin => {
                self.rust_handleDiagnosticsBegin(&params, &ctx)?
            }
            NOTIFICATION__RustDiagnosticsEnd => self.rust_handleDiagnosticsEnd(&params, &ctx)?,
            NOTIFICATION__WindowProgress => self.window_progress(&params, &ctx)?,
            NOTIFICATION__ServerExited => self.languageClient_serverExited(&params, &ctx)?,

            _ => {
                let languageId_target = if languageId.is_some() {
                    // Message from language server. No handler found.
                    let msg = format!("Message not handled: {:?}", notification);
                    if notification.method.starts_with('$') {
                        warn!("{}", msg);
                        return Ok(());
                    } else {
                        return Err(err_msg(msg));
                    }
                } else {
                    // Message from vim. Proxy to language server.
                    let (languageId_target,): (String,) =
                        self.gather_args(&[VimVar::LanguageId], &params)?;
                    info!(
                        "Proxy message directly to language server: {:?}",
                        notification
                    );
                    Some(languageId_target)
                };

                self.get_client(&languageId_target)?
                    .notify(&notification.method, &params)?;
            }
        };

        Ok(())
    }
}
