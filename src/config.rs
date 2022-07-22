use std::fs::{create_dir, OpenOptions};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

use dirs::config_dir;
use notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::time::TIMEOUT_1S;

pub struct ConfigManager<T: DeserializeOwned + Serialize + Default> {
    pub folder: Box<Path>,
    pub path: Box<Path>,
    pub config: T,
    watcher: RecommendedWatcher,
    watcher_output_rx: Receiver<DebouncedEvent>,
}

impl<T: DeserializeOwned + Serialize + Default> ConfigManager<T> {
    pub fn new(filename: &'static str) -> Self {
        let folder = config_dir()
            .expect("Unable to access the config folder.")
            .join("mad-rust");
        let path = folder.join(format!("{}.json", filename));

        create_dir(folder.clone()).ok();

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path.clone())
            .expect(format!("Unable to find or create the config file : {:?}", path).as_str());
        let mut config = T::default();

        if let Ok(_config) = serde_json::from_reader(&file) {
            config = _config;
        } else {
            serde_json::to_writer_pretty(file, &config).unwrap();
        }

        // watcher initialization
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = Watcher::new(tx, TIMEOUT_1S * 10).unwrap();

        watcher
            .watch(path.clone(), RecursiveMode::NonRecursive)
            .unwrap();

        Self {
            folder: folder.into_boxed_path(),
            path: path.into_boxed_path(),
            config,
            watcher,
            watcher_output_rx: rx,
        }
    }

    pub fn update(&mut self) -> bool {
        if let Ok(DebouncedEvent::Write(path)) = self.watcher_output_rx.try_recv() {
            if let Ok(file) = OpenOptions::new().read(true).open(path) {
                if let Ok(config) = serde_json::from_reader(&file) {
                    self.config = config;

                    return true;
                }
            }
        }

        false
    }

    pub fn save(&self) -> Option<()> {
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .open(self.path.clone())
        {
            serde_json::to_writer_pretty(file, &self.config).ok()
        } else {
            None
        }
    }

    pub fn close(&mut self) -> Option<()> {
        self.watcher.unwatch(self.path.clone()).ok();
        self.save()
    }
}
