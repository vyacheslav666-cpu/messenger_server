pub struct Config {
    pub port: u16,
    pub database_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);

        let database_path = std::env::var("DATABASE_PATH")
            .unwrap_or_else(|_| "data/chat.db".to_string());

        Self { port, database_path }
    }
}
