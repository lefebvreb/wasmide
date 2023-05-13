mod error;
mod raw;
mod traits;

use core::mem;
use core::ops::{Deref, DerefMut};

use alloc::rc::{Rc, Weak};

use self::raw::{RawSignal, SubscriberId};

pub use error::*;
pub use traits::*;

#[repr(transparent)]
pub struct Signal<T: 'static>(Rc<RawSignal<T>>);

impl<T> Signal<T> {
    #[inline]
    fn new_from_raw(raw: RawSignal<T>) -> Self {
        Self(Rc::new(raw))
    }

    #[inline]
    fn inner(&self) -> &Rc<RawSignal<T>> {
        &self.0
    }

    #[inline]
    pub fn try_get(&self) -> Result<T>
    where
        T: Clone,
    {
        self.inner().try_get()
    }

    #[inline]
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.try_get().unwrap()
    }

    pub fn for_each<F>(&self, notify: F) -> Unsubscriber<T>
    where
        F: FnMut(&T) + 'static,
    {
        let id = self.inner().raw_for_each(|_| notify);
        Unsubscriber::new(Rc::downgrade(self.inner()), id)
    }

    pub fn for_each_inner<F>(&self, mut notify: F)
    where
        F: FnMut(&T, &mut Unsubscriber<T>) + 'static,
    {
        let weak = Rc::downgrade(self.inner());
        self.inner().raw_for_each(|id| {
            let mut unsub = Unsubscriber::new(weak, id);
            move |data| notify(data, &mut unsub)
        });
    }

    #[inline]
    pub fn for_each_forever<F>(&self, notify: F)
    where
        F: FnMut(&T) + 'static,
    {
        self.inner().raw_for_each(|_| notify);
    }

    pub fn map<B, F>(&self, map: F) -> Signal<B>
    where
        F: FnMut(&T) -> B + 'static,
    {
        todo!()
    }

    pub fn filter<P>(&self, predicate: P) -> Signal<T>
    where
        P: FnMut(&T) -> bool,
    {
        todo!()
    }
}

impl<T> Clone for Signal<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[repr(transparent)]
pub struct Mutable<T: 'static>(Signal<T>);

impl<T> Mutable<T> {
    #[inline]
    pub fn new(initial_value: T) -> Self {
        Self(Signal::new_from_raw(RawSignal::new(initial_value)))
    }

    #[inline]
    pub fn uninit() -> Self {
        Self(Signal::new_from_raw(RawSignal::uninit()))
    }

    #[inline]
    pub fn try_set(&self, new_value: T) -> Result<()> {
        self.inner().try_set(new_value)
    }

    #[inline]
    pub fn set(&self, new_value: T) {
        self.try_set(new_value).unwrap();
    }

    #[inline]
    pub fn try_mutate<F>(&self, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut T),
    {
        self.inner().try_mutate(mutate)
    }

    #[inline]
    pub fn mutate<F>(&self, mutate: F)
    where
        F: FnOnce(&mut T),
    {
        self.try_mutate(mutate).unwrap();
    }

    #[inline]
    pub fn for_each<F>(&self, notify: F) -> Unsubscriber<T>
    where
        F: FnMut(&T) + 'static,
    {
        self.0.for_each(notify)
    }

    #[inline]
    pub fn for_each_inner<F>(&self, notify: F)
    where
        F: FnMut(&T, &mut Unsubscriber<T>) + 'static,
    {
        self.0.for_each_inner(notify);
    }

    #[inline]
    pub fn for_each_forever<F>(&self, notify: F)
    where
        F: FnMut(&T) + 'static,
    {
        self.0.for_each_forever(notify);
    }
}

impl<T> Clone for Mutable<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Deref for Mutable<T> {
    type Target = Signal<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> From<T> for Mutable<T> {
    #[inline]
    fn from(initial_value: T) -> Self {
        Self::new(initial_value)
    }
}

#[repr(transparent)]
pub struct Unsubscriber<T>(Option<(Weak<RawSignal<T>>, SubscriberId)>);

impl<T> Unsubscriber<T> {
    #[inline]
    fn new(weak: Weak<RawSignal<T>>, id: SubscriberId) -> Self {
        Self(Some((weak, id)))
    }

    pub fn unsubscribe(&mut self) {
        if let Some((weak, id)) = self.0.take() {
            if let Some(raw) = weak.upgrade() {
                raw.unsubscribe(id);
            }
        }
    }

    #[inline]
    pub fn has_effect(&self) -> bool {
        self.0.is_some()
    }
}

impl<T> Clone for Unsubscriber<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Clone)]
#[repr(transparent)]
pub struct DropUnsubscriber<T>(pub Unsubscriber<T>);

impl<T> DropUnsubscriber<T> {
    #[inline]
    pub fn take(self) -> Unsubscriber<T> {
        // SAFETY: `Self` and `Unsubscriber<T>` have the same `repr`.
        let inner = unsafe { mem::transmute_copy::<Self, Unsubscriber<T>>(&self) };
        mem::forget(self);
        inner
    }
}

impl<T> Deref for DropUnsubscriber<T> {
    type Target = Unsubscriber<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for DropUnsubscriber<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Drop for DropUnsubscriber<T> {
    #[inline]
    fn drop(&mut self) {
        self.unsubscribe()
    }
}

