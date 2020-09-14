use crate::common::rc::{PyRc, PyWeak};
use crate::pyobject::{IdProtocol, PyObject, PyObjectPayload};
use std::borrow;
use std::fmt;
use std::ops::Deref;

pub struct PyObjectRc<T = dyn PyObjectPayload>
where
    T: ?Sized + PyObjectPayload,
{
    inner: PyRc<PyObject<T>>,
}

pub type PyObjectWeak<T = dyn PyObjectPayload> = PyWeak<PyObject<T>>;

impl<T> PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
{
    pub fn into_raw(this: Self) -> *const PyObject<T> {
        let ptr = PyRc::as_ptr(&this.inner);
        std::mem::forget(this);
        ptr
    }

    /// # Safety
    /// See PyRc::from_raw
    pub unsafe fn from_raw(ptr: *const PyObject<T>) -> Self {
        Self {
            inner: PyRc::from_raw(ptr),
        }
    }

    pub fn new(value: PyObject<T>) -> Self
    where
        T: Sized,
    {
        Self {
            inner: PyRc::new(value),
        }
    }

    pub fn strong_count(this: &Self) -> usize {
        PyRc::strong_count(&this.inner)
    }

    pub fn weak_count(this: &Self) -> usize {
        PyRc::weak_count(&this.inner)
    }

    pub fn downgrade(this: &Self) -> PyObjectWeak<T> {
        PyRc::downgrade(&this.inner)
    }

    pub fn upgrade_weak(weak: &PyObjectWeak<T>) -> Option<Self> {
        weak.upgrade().map(|inner| PyObjectRc { inner })
    }
}

#[cfg(feature = "threading")]
unsafe impl<T> Send for PyObjectRc<T> where T: ?Sized + PyObjectPayload {}
#[cfg(feature = "threading")]
unsafe impl<T> Sync for PyObjectRc<T> where T: ?Sized + PyObjectPayload {}

impl<T> Deref for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
{
    type Target = PyObject<T>;

    #[inline]
    fn deref(&self) -> &PyObject<T> {
        self.inner.deref()
    }
}

impl<T> Clone for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
{
    fn clone(&self) -> Self {
        PyObjectRc {
            inner: self.inner.clone(),
        }
    }
}

impl<T> fmt::Display for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
    PyObject<T>: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T> fmt::Debug for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
    PyObject<T>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T> fmt::Pointer for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
    PyObject<T>: fmt::Pointer,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T> borrow::Borrow<T> for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
    PyRc<PyObject<T>>: borrow::Borrow<T>,
{
    fn borrow(&self) -> &T {
        self.inner.borrow()
    }
}

impl<T> AsRef<T> for PyObjectRc<T>
where
    T: ?Sized + PyObjectPayload,
    PyRc<PyObject<T>>: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.inner.as_ref()
    }
}

impl IdProtocol for PyObjectRc {
    fn get_id(&self) -> usize {
        self.inner.get_id()
    }
}