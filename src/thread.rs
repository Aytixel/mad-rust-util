use barrage::{unbounded, Disconnected, Receiver, RecvFut, SendFut, Sender, SharedReceiver};
use sysinfo::{get_current_pid, ProcessExt, ProcessRefreshKind, RefreshKind, System, SystemExt};

use std::env::current_exe;
use std::sync::{
    Condvar, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
    TryLockError, WaitTimeoutResult,
};
use std::time::Duration;

pub struct DualChannel<T: Clone + Unpin> {
    tx: Sender<T>,
    rx: Receiver<T>,
}

unsafe impl<T: Clone + Unpin> Send for DualChannel<T> {}
unsafe impl<T: Clone + Unpin> Sync for DualChannel<T> {}

impl<T: Clone + Unpin> Clone for DualChannel<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            rx: self.rx.clone(),
        }
    }
}

impl<T: Clone + Unpin> DualChannel<T> {
    pub fn new() -> (Self, Self) {
        let host = unbounded();
        let child = unbounded();

        (
            Self {
                tx: host.0,
                rx: child.1,
            },
            Self {
                tx: child.0,
                rx: host.1,
            },
        )
    }

    pub fn send(&self, item: T) -> Result<(), barrage::SendError<T>> {
        self.tx.send(item)
    }

    pub fn try_send(&self, item: T) -> Result<(), barrage::TrySendError<T>> {
        self.tx.try_send(item)
    }

    pub fn send_async(&self, item: T) -> SendFut<'_, T> {
        self.tx.send_async(item)
    }

    pub fn recv(&self) -> Result<T, Disconnected> {
        self.rx.recv()
    }

    pub fn try_recv(&self) -> Result<Option<T>, Disconnected> {
        self.rx.try_recv()
    }

    pub fn recv_async(&self) -> RecvFut<'_, T> {
        self.rx.recv_async()
    }

    pub fn into_shared(self) -> SharedReceiver<T> {
        self.rx.into_shared()
    }
}

pub fn kill_double() -> bool {
    if let Ok(path) = current_exe() {
        if let Ok(pid) = get_current_pid() {
            let mut sys = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new()),
            );

            sys.refresh_processes_specifics(ProcessRefreshKind::new());

            for (process_pid, process) in sys.processes() {
                if process.exe() == path && pid != *process_pid {
                    return true;
                }
            }
        }
    }

    false
}

pub trait MutexTrait<'a, T> {
    fn lock_poisoned(&self) -> MutexGuard<'_, T>;

    fn try_lock_poisoned(&self) -> Option<MutexGuard<'_, T>>;
}

impl<'a, T> MutexTrait<'_, T> for Mutex<T> {
    fn lock_poisoned(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn try_lock_poisoned(&self) -> Option<MutexGuard<'_, T>> {
        match self.try_lock() {
            Ok(guard) => Some(guard),
            Err(error) => match error {
                TryLockError::Poisoned(poisoned) => Some(poisoned.into_inner()),
                TryLockError::WouldBlock => None,
            },
        }
    }
}

pub trait RwLockTrait<'a, T> {
    fn read_poisoned(&self) -> RwLockReadGuard<'_, T>;

    fn try_read_poisoned(&self) -> Option<RwLockReadGuard<'_, T>>;

    fn write_poisoned(&self) -> RwLockWriteGuard<'_, T>;

    fn try_write_poisoned(&self) -> Option<RwLockWriteGuard<'_, T>>;
}

impl<'a, T> RwLockTrait<'_, T> for RwLock<T> {
    fn read_poisoned(&self) -> RwLockReadGuard<'_, T> {
        match self.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn try_read_poisoned(&self) -> Option<RwLockReadGuard<'_, T>> {
        match self.try_read() {
            Ok(guard) => Some(guard),
            Err(error) => match error {
                TryLockError::Poisoned(poisoned) => Some(poisoned.into_inner()),
                TryLockError::WouldBlock => None,
            },
        }
    }

    fn write_poisoned(&self) -> RwLockWriteGuard<'_, T> {
        match self.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn try_write_poisoned(&self) -> Option<RwLockWriteGuard<'_, T>> {
        match self.try_write() {
            Ok(guard) => Some(guard),
            Err(error) => match error {
                TryLockError::Poisoned(poisoned) => Some(poisoned.into_inner()),
                TryLockError::WouldBlock => None,
            },
        }
    }
}

pub trait CondvarTrait {
    fn wait_poisoned<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T>;

    fn wait_timeout_poisoned<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        dur: Duration,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult);

    fn wait_timeout_while_poisoned<'a, T, F: FnMut(&mut T) -> bool>(
        &self,
        guard: MutexGuard<'a, T>,
        dur: Duration,
        condition: F,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult);

    fn wait_while_poisoned<'a, T, F: FnMut(&mut T) -> bool>(
        &self,
        guard: MutexGuard<'a, T>,
        condition: F,
    ) -> MutexGuard<'a, T>;
}

impl CondvarTrait for Condvar {
    fn wait_poisoned<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        match self.wait(guard) {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn wait_timeout_poisoned<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        dur: Duration,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult) {
        match self.wait_timeout(guard, dur) {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn wait_timeout_while_poisoned<'a, T, F: FnMut(&mut T) -> bool>(
        &self,
        guard: MutexGuard<'a, T>,
        dur: Duration,
        condition: F,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult) {
        match self.wait_timeout_while(guard, dur, condition) {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn wait_while_poisoned<'a, T, F: FnMut(&mut T) -> bool>(
        &self,
        guard: MutexGuard<'a, T>,
        condition: F,
    ) -> MutexGuard<'a, T> {
        match self.wait_while(guard, condition) {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

pub enum CondMutexResult<'a, T> {
    Ok(MutexGuard<'a, T>),
    TimeoutOk((MutexGuard<'a, T>, WaitTimeoutResult)),
    Err(PoisonError<MutexGuard<'a, T>>),
    TimeoutErr(PoisonError<(MutexGuard<'a, T>, WaitTimeoutResult)>),
}
pub struct CondMutex<T>(Mutex<T>, Condvar);

impl<T> CondMutex<T> {
    pub fn new(value: T) -> Self {
        Self(Mutex::new(value), Condvar::new())
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, T>, PoisonError<MutexGuard<'_, T>>> {
        self.0.lock()
    }

    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, TryLockError<MutexGuard<'_, T>>> {
        self.0.try_lock()
    }

    pub fn lock_poisoned(&self) -> MutexGuard<'_, T> {
        self.0.lock_poisoned()
    }

    pub fn try_lock_poisoned(&self) -> Option<MutexGuard<'_, T>> {
        self.0.try_lock_poisoned()
    }

    pub fn wait(&self) -> CondMutexResult<T> {
        match self.0.lock() {
            Ok(guard) => match self.1.wait(guard) {
                Ok(guard) => CondMutexResult::Ok(guard),
                Err(poisoned) => CondMutexResult::Err(poisoned),
            },
            Err(poisoned) => CondMutexResult::Err(poisoned),
        }
    }

    pub fn wait_timeout(&self, dur: Duration) -> CondMutexResult<T> {
        match self.0.lock() {
            Ok(guard) => match self.1.wait_timeout(guard, dur) {
                Ok(guard) => CondMutexResult::TimeoutOk(guard),
                Err(poisoned) => CondMutexResult::TimeoutErr(poisoned),
            },
            Err(poisoned) => CondMutexResult::Err(poisoned),
        }
    }

    pub fn wait_timeout_while<F: FnMut(&mut T) -> bool>(
        &self,
        dur: Duration,
        condition: F,
    ) -> CondMutexResult<T> {
        match self.0.lock() {
            Ok(guard) => match self.1.wait_timeout_while(guard, dur, condition) {
                Ok(guard) => CondMutexResult::TimeoutOk(guard),
                Err(poisoned) => CondMutexResult::TimeoutErr(poisoned),
            },
            Err(poisoned) => CondMutexResult::Err(poisoned),
        }
    }

    pub fn wait_while<F: FnMut(&mut T) -> bool>(&self, condition: F) -> CondMutexResult<T> {
        match self.0.lock() {
            Ok(guard) => match self.1.wait_while(guard, condition) {
                Ok(guard) => CondMutexResult::Ok(guard),
                Err(poisoned) => CondMutexResult::Err(poisoned),
            },
            Err(poisoned) => CondMutexResult::Err(poisoned),
        }
    }

    pub fn wait_poisoned(&self) -> MutexGuard<'_, T> {
        self.1.wait_poisoned(self.0.lock_poisoned())
    }

    pub fn wait_timeout_poisoned(&self, dur: Duration) -> (MutexGuard<'_, T>, WaitTimeoutResult) {
        self.1.wait_timeout_poisoned(self.0.lock_poisoned(), dur)
    }

    pub fn wait_timeout_while_poisoned<F: FnMut(&mut T) -> bool>(
        &self,
        dur: Duration,
        condition: F,
    ) -> (MutexGuard<'_, T>, WaitTimeoutResult) {
        self.1
            .wait_timeout_while_poisoned(self.0.lock_poisoned(), dur, condition)
    }

    pub fn wait_while_poisoned<F: FnMut(&mut T) -> bool>(&self, condition: F) -> MutexGuard<'_, T> {
        self.1
            .wait_while_poisoned(self.0.lock_poisoned(), condition)
    }

    pub fn notify_one(&self) {
        self.1.notify_one();
    }

    pub fn notify_all(&self) {
        self.1.notify_all();
    }
}
