use crate::types::Bufnr;
use crate::viewport::Viewport;
use crate::vim::Vim;
use failure::Fallible;
use jsonrpc_core::Params;
use lazycell::LazyCell;
use lsp_types::Position;
use serde::de::DeserializeOwned;
use serde_json::Map;
use serde_json::Value;

pub struct Context {
    vim: Vim,
    bufname: LazyCell<String>,
    bufnr: LazyCell<Bufnr>,
    language_id: LazyCell<String>,
    viewport: LazyCell<Viewport>,
    position: LazyCell<Position>,
    current_word: LazyCell<String>,
    text: LazyCell<Vec<String>>,
    handle: LazyCell<bool>,
    precalc: Map<String, Value>,
}

impl Context {
    pub fn new(vim: Vim, params: &Params) -> Self {
        let precalc = if let Params::Map(map) = params {
            map.clone()
        } else {
            Map::default()
        };

        Context {
            vim,
            bufname: LazyCell::new(),
            bufnr: LazyCell::new(),
            language_id: LazyCell::new(),
            viewport: LazyCell::new(),
            position: LazyCell::new(),
            current_word: LazyCell::new(),
            text: LazyCell::new(),
            handle: LazyCell::new(),
            precalc,
        }
    }

    /// Try get value from precalc.
    pub fn try_get<R: DeserializeOwned>(&self, key: &str) -> Fallible<Option<R>> {
        if let Some(value) = self.precalc.get(key) {
            Ok(Some(serde_json::from_value(value.clone())?))
        } else {
            Ok(None)
        }
    }

    pub fn get_filename(&self) -> Fallible<&String> {
        self.bufname.try_borrow_with(|| {
            self.try_get("filename")?
                .map_or_else(|| self.vim.eval("LSP#filename()"), Ok)
        })
    }

    pub fn get_bufnr(&self) -> Fallible<&Bufnr> {
        self.bufnr.try_borrow_with(|| {
            self.try_get("bufnr")?.map_or_else(
                || self.vim.eval(format!("bufnr('{}')", self.get_filename()?)),
                Ok,
            )
        })
    }

    pub fn get_languageId(&self) -> Fallible<&String> {
        self.language_id.try_borrow_with(|| {
            self.try_get("languageId")?
                .map_or_else(|| self.vim.getbufvar(self.get_filename()?, "&filetype"), Ok)
        })
    }

    pub fn get_viewport(&self) -> Fallible<&Viewport> {
        let expr = "LSP#viewport()";

        self.viewport
            .try_borrow_with(|| self.try_get(expr)?.map_or_else(|| self.vim.eval(expr), Ok))
    }

    pub fn get_position(&self) -> Fallible<&Position> {
        let expr = "LSP#position()";

        self.position
            .try_borrow_with(|| self.try_get(expr)?.map_or_else(|| self.vim.eval(expr), Ok))
    }

    pub fn get_current_word(&self) -> Fallible<&String> {
        let expr = "expand('<cword>')";

        self.current_word
            .try_borrow_with(|| self.try_get(expr)?.map_or_else(|| self.vim.eval(expr), Ok))
    }

    pub fn get_text(&self, start: &str, end: &str) -> Fallible<&Vec<String>> {
        self.text
            .try_borrow_with(|| self.vim.getbufline(self.get_filename()?, start, end))
    }

    pub fn get_handle(&self) -> Fallible<&bool> {
        self.handle
            .try_borrow_with(|| self.try_get("handle")?.map_or_else(|| Ok(true), Ok))
    }
}
