use crate::db;
use rusqlite::Connection;
use std::{collections::HashMap, fs, sync::{Arc, Mutex}};
use tokio::sync::mpsc;

pub type ActiveConnections = Arc<Mutex<HashMap<i64, mpsc::UnboundedSender<String>>>>;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub conns: ActiveConnections,
}

impl AppState {
    pub fn new(database_path: &str) -> Self {
        if let Some(parent) = std::path::Path::new(database_path).parent() {
            fs::create_dir_all(parent).expect("failed to create data directory");
        }

        let conn = Connection::open(database_path).expect("failed to open database");
        db::init(&conn).expect("failed to init database");

        Self {
            db: Mutex::new(conn),
            conns: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
