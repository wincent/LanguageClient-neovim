#[allow(non_snake_case)]

use std::sync::Mutex;
use std::env::current_dir;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
extern crate lazy_static;
extern crate neovim_lib;

use neovim_lib::{Neovim, NeovimApi, Session, Value};

lazy_static::lazy_static! {
    static ref NVIM: Mutex<Neovim> = {
        let mut session = Session::new_unix_socket("/tmp/nvim-LanguageClient-IntegrationTest").unwrap();
        session.start_event_loop();
        Neovim::new(session).into()
    };

    static ref INDEXJS: String = {
        current_dir().unwrap().join("data/sample-js/src/index.js")
            .to_string_lossy().into()
    };
}

trait NeovimApiExt {
    fn edit(&mut self, p: impl AsRef<Path>);
    fn cursor(&mut self, lnum: usize, col: usize);
}

impl NeovimApiExt for Neovim {
    fn edit(&mut self, p: impl AsRef<Path>) {
        self.command(&format!("edit! {}", p.as_ref().to_string_lossy()));
    }

    fn cursor(&mut self, lnum: usize, col: usize) {
        self.call_function("cursor", vec![lnum.into(), col.into()]).unwrap();
    }
}

#[test]
fn test_LanguageClient_textDocument_hover() {
    let mut nvim = NVIM.lock().unwrap();

    nvim.command(&format!("edit! {}", *INDEXJS)).unwrap();
    sleep(Duration::from_secs(1));
    nvim.cursor(13, 19);
}
