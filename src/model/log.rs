use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

// 自定义日志文件结构体，处理日期检查和文件切换
struct DailyFile {
    base_path: String,
    current_date: String,
    file: File,
}

impl DailyFile {
    pub fn new(base_path: &str) -> io::Result<Self> {
        let log_file_dir = PathBuf::from(base_path);
        if !log_file_dir.exists() {
            std::fs::create_dir_all(log_file_dir)?;
        }
        let date = Local::now().format("%Y-%m-%d").to_string();
        let path = format!("{}/{}.log", base_path, date);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            base_path: base_path.to_string(),
            current_date: date,
            file,
        })
    }

    // 检查日期，必要时创建新文件
    fn check_date(&mut self) -> io::Result<()> {
        let new_date = Local::now().format("%Y-%m-%d").to_string();
        if self.current_date != new_date {
            let path = format!("{}/{}.log", self.base_path, new_date);
            let new_file = OpenOptions::new().create(true).append(true).open(path)?;
            self.file = new_file;
            self.current_date = new_date;
        }
        Ok(())
    }
}

// 实现Write trait，处理日志写入
impl Write for DailyFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.check_date()?;
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

// 线程安全适配器，包装DailyFile以支持多线程写入
pub struct DailyFileAdapter {
    inner: Mutex<DailyFile>,
}

impl DailyFileAdapter {
    pub fn new(base_path: &str) -> io::Result<Self> {
        let daily_file = DailyFile::new(base_path)?;
        Ok(Self {
            inner: Mutex::new(daily_file),
        })
    }
}

// 为适配器实现Write trait
impl Write for DailyFileAdapter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.flush()
    }
}
