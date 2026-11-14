//! One TubeAmp per config folder.
//!
//! Two copies sharing a config folder both read the same saved window positions, and a second copy
//! started before the first has saved anything places its windows at the defaults. Both then open
//! their main window at the same spot, which looks like one window drawn twice. A lock file in the
//! config folder, held with an advisory `flock`-style lock, stops the second copy. The OS releases
//! the lock when the process exits, however it exits, so a crash never leaves it stuck.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// The lock file's name in the config folder.
const LOCK_FILE: &str = "instance.lock";

/// The lock, held for as long as this value lives.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// Whether this copy got the lock.
#[derive(Debug)]
pub enum Acquired {
    /// This is the only copy using the folder. Keep the lock alive until exit.
    Held(InstanceLock),
    /// Another copy holds the lock.
    Taken,
}

/// Takes the lock in `dir`, creating the folder and the file if needed.
pub fn acquire(dir: &Path) -> io::Result<Acquired> {
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(dir.join(LOCK_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(Acquired::Held(InstanceLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(Acquired::Taken),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tubeamp-instance-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_second_copy_in_the_same_folder_is_refused_until_the_first_lets_go() {
        let dir = test_dir("second");
        let first = match acquire(&dir).expect("first copy") {
            Acquired::Held(lock) => lock,
            Acquired::Taken => panic!("the first copy must get the lock"),
        };
        assert!(dir.join(LOCK_FILE).is_file());
        assert!(matches!(
            acquire(&dir).expect("second copy"),
            Acquired::Taken
        ));
        drop(first);
        assert!(matches!(
            acquire(&dir).expect("after exit"),
            Acquired::Held(_)
        ));
    }

    #[test]
    fn different_folders_do_not_block_each_other() {
        let a = test_dir("a");
        let b = test_dir("b");
        let _lock_a = acquire(&a).expect("a");
        assert!(matches!(acquire(&b).expect("b"), Acquired::Held(_)));
    }
}
