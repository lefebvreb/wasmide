use core::cell::{Cell, UnsafeCell};
use core::num::NonZeroU32;
use core::ops::{Deref, DerefMut};

use alloc::boxed::Box;
use alloc::rc::{Rc, Weak};
use alloc::vec::Vec;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SignalUpdatingError;

#[derive(Copy, Clone, Debug)]
enum NotifierState {
    Active(NonZeroU32),
    Deleted(NonZeroU32),
}

#[derive(Debug)]
struct Notifier {
    state: Cell<NotifierState>,
    notify: *mut dyn FnMut(),
}

impl Drop for Notifier {
    #[inline]
    fn drop(&mut self) {
        unsafe { 
            let _ = Box::from_raw(self.notify); 
        }
    }
}

impl Notifier {
    #[inline]
    fn id(&self) -> NonZeroU32 {
        match self.state.get() {
            NotifierState::Active(id) | NotifierState::Deleted(id) => id,
        }
    }

    #[inline]
    fn deleted(&self) -> bool {
        matches!(self.state.get(), NotifierState::Deleted(_))
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum SignalState {
    /// The signal is not currently in use.
    Idling,
    /// The signal's data is currently being updated and/or
    /// its subscribers are being notified.
    Mutating,
    /// The signal is currently notifying new susbcribers with
    /// a reference to the current data.
    Subscribing,
}

#[derive(Debug)]
struct RawSignal {
    state: Cell<SignalState>,
    subscribers: UnsafeCell<Vec<Notifier>>,
    next_id: Cell<NonZeroU32>,
    needs_delete: Cell<bool>,
}

impl RawSignal {
    #[inline]
    fn new() -> Self {
        Self {
            state: Cell::new(SignalState::Idling),
            subscribers: UnsafeCell::new(Vec::new()),
            next_id: Cell::new(1.try_into().unwrap()),
            needs_delete: Cell::new(false),
        }
    }

    /// # Safety
    ///
    /// The caller must ensure that no contained `self.subscribers[i].notify` gets
    /// called or dropped.
    unsafe fn notify_all(&self) {
        let subscribers = self.subscribers.get();
        let mut i = 0;

        while i < (*subscribers).len() {
            let notifier = (*subscribers).as_mut_ptr().add(i).as_ref().unwrap();

            if !notifier.deleted() {
                (*notifier.notify)();
            }

            i += 1;
        }

        if self.needs_delete.take() {
            (*subscribers).retain(Notifier::deleted);
        }
    }

    fn try_mutate(&self, f: impl FnOnce()) -> Result<(), SignalUpdatingError> {
        if self.state.get() != SignalState::Idling {
            return Err(SignalUpdatingError);
        }

        self.state.set(SignalState::Mutating);

        f();

        // SAFETY: we set the state to Mutating, therefore preventing
        // a second call to this function until this one terminates.
        unsafe { self.notify_all() };

        self.state.set(SignalState::Idling);
        Ok(())
    }

    fn for_each(&self, mut f: Box<dyn FnMut()>) -> NonZeroU32 {
        if self.state.get() != SignalState::Mutating {
            let old_state = self.state.replace(SignalState::Subscribing);
            f();
            self.state.set(old_state);
        }

        let id = self.next_id.get();
        self.next_id.set(id.saturating_add(1));

        let subscribers = self.subscribers.get();

        unsafe {
            (*subscribers).push(Notifier {
                state: Cell::new(NotifierState::Active(id)),
                notify: Box::into_raw(f),
            });
        }

        id
    }

    fn unsubscribe(&self, id: NonZeroU32) {
        let subscribers = self.subscribers.get();

        unsafe {
            if let Ok(index) = (*subscribers).binary_search_by_key(&id, Notifier::id) {
                if self.state.get() == SignalState::Mutating {
                    let notifier = (*subscribers).get(index).unwrap();
                    let id = notifier.id();
                    notifier.state.set(NotifierState::Deleted(id));
                    self.needs_delete.set(true);
                } else {
                    (*subscribers).remove(index);
                }
            }
        }
    }
}

#[derive(Debug)]
struct InnerSignal<T>(RawSignal, UnsafeCell<T>);

impl<T> InnerSignal<T> {
    #[inline]
    fn get(&self) -> (&RawSignal, *mut T) {
        (&self.0, self.1.get())
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Signal<T: 'static>(Rc<InnerSignal<T>>);

impl<T> Signal<T> {
    pub fn new(value: T) -> Self {
        let raw = RawSignal::new();
        let data = UnsafeCell::new(value);
        Self(Rc::new(InnerSignal(raw, data)))
    }

    #[inline]
    pub fn try_mutate(&self, f: impl FnOnce(&mut T)) -> Result<(), SignalUpdatingError> {
        let (raw, data) = self.0.get();
        raw.try_mutate(|| f(unsafe { &mut *data }))
    }

    #[inline]
    pub fn try_update(&self, f: impl FnOnce(&T) -> T) -> Result<(), SignalUpdatingError> {
        self.try_mutate(|data| *data = f(data))
    }

    #[inline]
    pub fn try_set(&self, value: T) -> Result<(), SignalUpdatingError> {
        self.try_mutate(|data| *data = value)
    }

    #[inline]
    pub fn mutate(&self, f: impl FnOnce(&mut T)) {
        self.try_mutate(f).unwrap();
    }

    #[inline]
    pub fn update(&self, f: impl FnOnce(&T) -> T) {
        self.try_update(f).unwrap();
    }

    #[inline]
    pub fn set(&self, value: T) {
        self.try_set(value).unwrap();
    }

    pub fn for_each(&self, mut f: impl FnMut(&T) + 'static) -> Unsubscriber<T> {
        let (raw, data) = self.0.get();
        let notify = move || f(unsafe { &*data });
        let id = raw.for_each(Box::new(notify));

        let info = NotifierRef {
            signal: Rc::downgrade(&self.0),
            id,
        };

        Unsubscriber(Some(info))
    }
}

impl<T: Clone> Signal<T> {
    #[inline]
    pub fn get(&self) -> T {
        let (_, data) = self.0.get();
        unsafe { (*data).clone() }
    }
}

impl<T> Clone for Signal<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Computed<T: 'static>(Signal<T>);

#[derive(Debug)]
struct NotifierRef<T> {
    signal: Weak<InnerSignal<T>>,
    id: NonZeroU32,
}

impl<T> Clone for NotifierRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
            id: self.id,
        }
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Unsubscriber<T>(Option<NotifierRef<T>>);

impl<T> Unsubscriber<T> {
    #[inline]
    pub fn needed(&self) -> bool {
        self.0.is_some()
    }

    pub fn unsubscribe(&mut self) {
        if let Some(info) = self.0.take() {
            if let Some(inner) = info.signal.upgrade() {
                let (raw, _) = inner.get();
                raw.unsubscribe(info.id);
            }
        }
    }

    #[inline]
    pub fn droppable(self) -> DroppableUnsubscriber<T> {
        DroppableUnsubscriber(self)
    }
}

impl<T> Clone for Unsubscriber<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct DroppableUnsubscriber<T>(pub Unsubscriber<T>);

impl<T> DroppableUnsubscriber<T> {
    #[inline]
    pub fn take(mut self) -> Unsubscriber<T> {
        Unsubscriber(self.0.0.take())
    }
}

impl<T> Clone for DroppableUnsubscriber<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Deref for DroppableUnsubscriber<T> {
    type Target = Unsubscriber<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for DroppableUnsubscriber<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Drop for DroppableUnsubscriber<T> {
    #[inline]
    fn drop(&mut self) {
        self.0.unsubscribe();
    }
}

pub trait Value<T> {
    fn for_each(&self, f: impl FnMut(&T) + 'static) -> Unsubscriber<T>;

    fn subscribe_inner(&self, f: impl FnMut(&T, &mut Unsubscriber<T>) + 'static) {
        todo!()
    }
}

impl<T> Value<T> for T {
    #[inline]
    fn for_each(&self, mut f: impl FnMut(&T) + 'static) -> Unsubscriber<T> {
        f(self);
        Unsubscriber(None)
    }
}

impl<T: 'static> Value<T> for Signal<T> {
    #[inline]
    fn for_each(&self, f: impl FnMut(&T) + 'static) -> Unsubscriber<T> {
        self.for_each(f)
    }
}
