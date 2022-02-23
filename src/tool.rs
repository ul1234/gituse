use std::{fs, io::Write};

const LOG_TO_FILE_ENABLE: bool = false;
const LOG_FILE_NAME: &str = "test.txt";

pub fn log_init() {
    if LOG_TO_FILE_ENABLE {
        if let Err(err) = fs::remove_file(LOG_FILE_NAME) {
            println!("log file delete: {}", err);
        }
    }
}

pub fn log<S: Into<String>>(info: S) {
    if LOG_TO_FILE_ENABLE {
        let mut file =
            fs::OpenOptions::new().write(true).append(true).create(true).open(LOG_FILE_NAME).expect("log file open failed!");
        file.write_all(info.into().as_bytes()).unwrap();
    }
}
