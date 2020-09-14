use std::fmt;

use crossbeam_utils::atomic::AtomicCell;

use super::objiter;
use super::objset::PySet;
use super::objstr;
use super::objtype::{self, PyClassRef};
use crate::dictdatatype::{self, DictKey};
use crate::exceptions::PyBaseExceptionRef;
use crate::function::{KwArgs, OptionalArg, PyFuncArgs};
use crate::pyobject::{
    BorrowValue, IdProtocol, IntoPyObject, ItemProtocol, PyArithmaticValue, PyAttributes,
    PyClassImpl, PyComparisonValue, PyContext, PyIterable, PyObjectRef, PyRef, PyResult, PyValue,
    TryFromObject, TypeProtocol,
};
use crate::slots::{Comparable, PyComparisonOp};
use crate::vm::{ReprGuard, VirtualMachine};

use std::mem::size_of;
use PyArithmaticValue::{Implemented, NotImplemented};

pub type DictContentType = dictdatatype::Dict;

/// dict() -> new empty dictionary
/// dict(mapping) -> new dictionary initialized from a mapping object's
///    (key, value) pairs
/// dict(iterable) -> new dictionary initialized as if via:
///    d = {}
///    for k, v in iterable:
///        d[k] = v
/// dict(**kwargs) -> new dictionary initialized with the name=value pairs
///    in the keyword argument list.  For example:  dict(one=1, two=2)
#[pyclass(module = false, name = "dict")]
#[derive(Default)]
pub struct PyDict {
    entries: DictContentType,
}
pub type PyDictRef = PyRef<PyDict>;

impl fmt::Debug for PyDict {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: implement more detailed, non-recursive Debug formatter
        f.write_str("dict")
    }
}

impl PyValue for PyDict {
    fn class(vm: &VirtualMachine) -> PyClassRef {
        vm.ctx.types.dict_type.clone()
    }
}

// Python dict methods:
#[allow(clippy::len_without_is_empty)]
#[pyimpl(flags(BASETYPE))]
impl PyDict {
    #[pyslot]
    fn tp_new(class: PyClassRef, _args: PyFuncArgs, vm: &VirtualMachine) -> PyResult<PyRef<Self>> {
        PyDict {
            entries: DictContentType::default(),
        }
        .into_ref_with_type(vm, class)
    }

    #[pymethod(magic)]
    fn init(
        &self,
        dict_obj: OptionalArg<PyObjectRef>,
        kwargs: KwArgs,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        Self::merge(&self.entries, dict_obj, kwargs, vm)
    }

    fn merge(
        dict: &DictContentType,
        dict_obj: OptionalArg<PyObjectRef>,
        kwargs: KwArgs,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        if let OptionalArg::Present(dict_obj) = dict_obj {
            let dicted: Result<PyDictRef, _> = dict_obj.clone().downcast();
            if let Ok(dict_obj) = dicted {
                for (key, value) in dict_obj {
                    dict.insert(vm, key, value)?;
                }
            } else if let Some(keys) = vm.get_method(dict_obj.clone(), "keys") {
                let keys = objiter::get_iter(vm, &vm.invoke(&keys?, vec![])?)?;
                while let Some(key) = objiter::get_next_object(vm, &keys)? {
                    let val = dict_obj.get_item(key.clone(), vm)?;
                    dict.insert(vm, key, val)?;
                }
            } else {
                let iter = objiter::get_iter(vm, &dict_obj)?;
                loop {
                    fn err(vm: &VirtualMachine) -> PyBaseExceptionRef {
                        vm.new_type_error("Iterator must have exactly two elements".to_owned())
                    }
                    let element = match objiter::get_next_object(vm, &iter)? {
                        Some(obj) => obj,
                        None => break,
                    };
                    let elem_iter = objiter::get_iter(vm, &element)?;
                    let key = objiter::get_next_object(vm, &elem_iter)?.ok_or_else(|| err(vm))?;
                    let value = objiter::get_next_object(vm, &elem_iter)?.ok_or_else(|| err(vm))?;
                    if objiter::get_next_object(vm, &elem_iter)?.is_some() {
                        return Err(err(vm));
                    }
                    dict.insert(vm, key, value)?;
                }
            }
        }

        for (key, value) in kwargs.into_iter() {
            dict.insert(vm, vm.ctx.new_str(key), value)?;
        }
        Ok(())
    }

    fn merge_dict(
        dict: &DictContentType,
        dict_other: PyDictRef,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        for (key, value) in dict_other {
            dict.insert(vm, key, value)?;
        }
        Ok(())
    }

    #[pyclassmethod]
    fn fromkeys(
        class: PyClassRef,
        iterable: PyIterable,
        value: OptionalArg<PyObjectRef>,
        vm: &VirtualMachine,
    ) -> PyResult<PyRef<Self>> {
        let dict = DictContentType::default();
        let value = value.unwrap_or_else(|| vm.ctx.none());
        for elem in iterable.iter(vm)? {
            let elem = elem?;
            dict.insert(vm, elem, value.clone())?;
        }
        PyDict { entries: dict }.into_ref_with_type(vm, class)
    }

    #[pymethod(magic)]
    fn bool(&self) -> bool {
        !self.entries.is_empty()
    }

    fn inner_cmp(
        zelf: PyRef<Self>,
        other: PyDictRef,
        op: PyComparisonOp,
        item: bool,
        vm: &VirtualMachine,
    ) -> PyResult<PyComparisonValue> {
        if !op.eval_ord(zelf.len().cmp(&other.len())) {
            return Ok(Implemented(false));
        }
        let (zelf, other) = if zelf.len() < other.len() {
            (other, zelf)
        } else {
            (zelf, other)
        };
        for (k, v1) in other {
            match zelf.get_item_option(k, vm)? {
                Some(v2) => {
                    if v1.is(&v2) {
                        continue;
                    }
                    if item && !vm.bool_eq(v1, v2)? {
                        return Ok(Implemented(false));
                    }
                }
                None => {
                    return Ok(Implemented(false));
                }
            }
        }
        Ok(Implemented(true))
    }

    #[pymethod(magic)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    #[pymethod(magic)]
    fn sizeof(&self) -> usize {
        size_of::<Self>() + self.entries.sizeof()
    }

    #[pymethod(magic)]
    fn repr(zelf: PyRef<Self>, vm: &VirtualMachine) -> PyResult<String> {
        let s = if let Some(_guard) = ReprGuard::enter(vm, zelf.as_object()) {
            let mut str_parts = vec![];
            for (key, value) in zelf {
                let key_repr = vm.to_repr(&key)?;
                let value_repr = vm.to_repr(&value)?;
                str_parts.push(format!(
                    "{}: {}",
                    key_repr.borrow_value(),
                    value_repr.borrow_value()
                ));
            }

            format!("{{{}}}", str_parts.join(", "))
        } else {
            "{...}".to_owned()
        };
        Ok(s)
    }

    #[pymethod(magic)]
    fn contains(&self, key: PyObjectRef, vm: &VirtualMachine) -> PyResult<bool> {
        self.entries.contains(vm, &key)
    }

    #[pymethod(magic)]
    fn delitem(&self, key: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        self.entries.delete(vm, key)
    }

    #[pymethod]
    fn clear(&self) {
        self.entries.clear()
    }

    #[pymethod(magic)]
    fn iter(zelf: PyRef<Self>) -> PyDictKeyIterator {
        PyDictKeyIterator::new(zelf)
    }

    #[pymethod]
    fn keys(zelf: PyRef<Self>) -> PyDictKeys {
        PyDictKeys::new(zelf)
    }

    #[pymethod]
    fn values(zelf: PyRef<Self>) -> PyDictValues {
        PyDictValues::new(zelf)
    }

    #[pymethod]
    fn items(zelf: PyRef<Self>) -> PyDictItems {
        PyDictItems::new(zelf)
    }

    #[pymethod(magic)]
    fn setitem(&self, key: PyObjectRef, value: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        self.inner_setitem_fast(key, value, vm)
    }

    /// Set item variant which can be called with multiple
    /// key types, such as str to name a notable one.
    fn inner_setitem_fast<K: DictKey>(
        &self,
        key: K,
        value: PyObjectRef,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        self.entries.insert(vm, key, value)
    }

    #[pymethod(magic)]
    #[cfg_attr(feature = "flame-it", flame("PyDictRef"))]
    fn getitem(zelf: PyRef<Self>, key: PyObjectRef, vm: &VirtualMachine) -> PyResult {
        if let Some(value) = zelf.inner_getitem_option(key.clone(), vm)? {
            Ok(value)
        } else {
            Err(vm.new_key_error(key))
        }
    }

    #[pymethod]
    fn get(
        &self,
        key: PyObjectRef,
        default: OptionalArg<PyObjectRef>,
        vm: &VirtualMachine,
    ) -> PyResult {
        match self.entries.get(vm, &key)? {
            Some(value) => Ok(value),
            None => Ok(default.unwrap_or_else(|| vm.ctx.none())),
        }
    }

    #[pymethod]
    fn setdefault(
        &self,
        key: PyObjectRef,
        default: OptionalArg<PyObjectRef>,
        vm: &VirtualMachine,
    ) -> PyResult {
        match self.entries.get(vm, &key)? {
            Some(value) => Ok(value),
            None => {
                let set_value = default.unwrap_or_else(|| vm.ctx.none());
                self.entries.insert(vm, key, set_value.clone())?;
                Ok(set_value)
            }
        }
    }

    #[pymethod]
    pub fn copy(&self) -> PyDict {
        PyDict {
            entries: self.entries.clone(),
        }
    }

    #[pymethod]
    fn update(
        &self,
        dict_obj: OptionalArg<PyObjectRef>,
        kwargs: KwArgs,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        PyDict::merge(&self.entries, dict_obj, kwargs, vm)
    }

    #[pymethod(name = "__ior__")]
    fn ior(zelf: PyRef<Self>, other: PyObjectRef, vm: &VirtualMachine) -> PyResult {
        let dicted: Result<PyDictRef, _> = other.downcast();
        if let Ok(other) = dicted {
            PyDict::merge_dict(&zelf.entries, other, vm)?;
            return Ok(zelf.into_object());
        }
        Err(vm.new_type_error("__ior__ not implemented for non-dict type".to_owned()))
    }

    #[pymethod(name = "__ror__")]
    fn ror(zelf: PyRef<Self>, other: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyDict> {
        let dicted: Result<PyDictRef, _> = other.downcast();
        if let Ok(other) = dicted {
            let other_cp = other.copy();
            PyDict::merge_dict(&other_cp.entries, zelf, vm)?;
            return Ok(other_cp);
        }
        Err(vm.new_type_error("__ror__ not implemented for non-dict type".to_owned()))
    }

    #[pymethod(name = "__or__")]
    fn or(&self, other: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyDict> {
        let dicted: Result<PyDictRef, _> = other.downcast();
        if let Ok(other) = dicted {
            let self_cp = self.copy();
            PyDict::merge_dict(&self_cp.entries, other, vm)?;
            return Ok(self_cp);
        }
        Err(vm.new_type_error("__or__ not implemented for non-dict type".to_owned()))
    }

    #[pymethod]
    fn pop(
        &self,
        key: PyObjectRef,
        default: OptionalArg<PyObjectRef>,
        vm: &VirtualMachine,
    ) -> PyResult {
        match self.entries.pop(vm, &key)? {
            Some(value) => Ok(value),
            None => match default {
                OptionalArg::Present(default) => Ok(default),
                OptionalArg::Missing => Err(vm.new_key_error(key)),
            },
        }
    }

    #[pymethod]
    fn popitem(&self, vm: &VirtualMachine) -> PyResult {
        if let Some((key, value)) = self.entries.pop_front() {
            Ok(vm.ctx.new_tuple(vec![key, value]))
        } else {
            let err_msg = vm.ctx.new_str("popitem(): dictionary is empty");
            Err(vm.new_key_error(err_msg))
        }
    }

    pub fn from_attributes(attrs: PyAttributes, vm: &VirtualMachine) -> PyResult<Self> {
        let dict = DictContentType::default();

        for (key, value) in attrs {
            dict.insert(vm, vm.ctx.new_str(key), value)?;
        }

        Ok(PyDict { entries: dict })
    }

    #[pymethod(magic)]
    fn hash(&self, vm: &VirtualMachine) -> PyResult<()> {
        Err(vm.new_type_error("unhashable type".to_owned()))
    }

    pub fn contains_key<T: IntoPyObject>(&self, key: T, vm: &VirtualMachine) -> bool {
        let key = key.into_pyobject(vm);
        self.entries.contains(vm, &key).unwrap()
    }

    pub fn size(&self) -> dictdatatype::DictSize {
        self.entries.size()
    }

    #[pymethod(name = "__reversed__")]
    fn reversed(zelf: PyRef<Self>) -> PyDictReverseKeyIterator {
        PyDictReverseKeyIterator::new(zelf)
    }
}

impl Comparable for PyDict {
    fn cmp(
        zelf: PyRef<Self>,
        other: PyObjectRef,
        op: PyComparisonOp,
        vm: &VirtualMachine,
    ) -> PyResult<PyComparisonValue> {
        op.eq_only(|| {
            if let Ok(other) = other.downcast::<PyDict>() {
                Self::inner_cmp(zelf, other, PyComparisonOp::Eq, true, vm)
            } else {
                Ok(PyComparisonValue::NotImplemented)
            }
        })
    }
}

impl PyDictRef {
    /// Return an optional inner item, or an error (can be key error as well)
    fn inner_getitem_option<K: DictKey>(
        &self,
        key: K,
        vm: &VirtualMachine,
    ) -> PyResult<Option<PyObjectRef>> {
        if let Some(value) = self.entries.get(vm, &key)? {
            return Ok(Some(value));
        }

        if let Some(method_or_err) = vm.get_method(self.clone().into_object(), "__missing__") {
            let method = method_or_err?;
            Ok(Some(vm.invoke(&method, vec![key.into_pyobject(vm)])?))
        } else {
            Ok(None)
        }
    }

    /// Take a python dictionary and convert it to attributes.
    pub fn to_attributes(self) -> PyAttributes {
        let mut attrs = PyAttributes::new();
        for (key, value) in self {
            let key = objstr::clone_value(&key);
            attrs.insert(key, value);
        }
        attrs
    }

    /// This function can be used to get an item without raising the
    /// KeyError, so we can simply check upon the result being Some
    /// python value, or None.
    /// Note that we can pass any type which implements the DictKey
    /// trait. Notable examples are String and PyObjectRef.
    pub fn get_item_option<T: IntoPyObject + DictKey>(
        &self,
        key: T,
        vm: &VirtualMachine,
    ) -> PyResult<Option<PyObjectRef>> {
        // Test if this object is a true dict, or mabye a subclass?
        // If it is a dict, we can directly invoke inner_get_item_option,
        // and prevent the creation of the KeyError exception.
        // Also note, that we prevent the creation of a full PyString object
        // if we lookup local names (which happens all of the time).
        if self.lease_class().is(&vm.ctx.types.dict_type) {
            // We can take the short path here!
            match self.inner_getitem_option(key, vm) {
                Err(exc) => {
                    if objtype::isinstance(&exc, &vm.ctx.exceptions.key_error) {
                        Ok(None)
                    } else {
                        Err(exc)
                    }
                }
                Ok(x) => Ok(x),
            }
        } else {
            // Fall back to full get_item with KeyError checking

            match self.get_item(key, vm) {
                Ok(value) => Ok(Some(value)),
                Err(exc) => {
                    if objtype::isinstance(&exc, &vm.ctx.exceptions.key_error) {
                        Ok(None)
                    } else {
                        Err(exc)
                    }
                }
            }
        }
    }
}

impl<T> ItemProtocol<T> for PyDictRef
where
    T: DictKey,
{
    fn get_item(&self, key: T, vm: &VirtualMachine) -> PyResult {
        self.as_object().get_item(key, vm)
    }

    fn set_item(&self, key: T, value: PyObjectRef, vm: &VirtualMachine) -> PyResult {
        if self.lease_class().is(&vm.ctx.types.dict_type) {
            self.inner_setitem_fast(key, value, vm)
                .map(|_| vm.ctx.none())
        } else {
            // Fall back to slow path if we are in a dict subclass:
            self.as_object().set_item(key, value, vm)
        }
    }

    fn del_item(&self, key: T, vm: &VirtualMachine) -> PyResult {
        if self.lease_class().is(&vm.ctx.types.dict_type) {
            self.entries.delete(vm, key).map(|_| vm.ctx.none())
        } else {
            // Fall back to slow path if we are in a dict subclass:
            self.as_object().del_item(key, vm)
        }
    }
}

// Implement IntoIterator so that we can easily iterate dictionaries from rust code.
impl IntoIterator for PyDictRef {
    type Item = (PyObjectRef, PyObjectRef);
    type IntoIter = DictIter;

    fn into_iter(self) -> Self::IntoIter {
        DictIter::new(self)
    }
}

impl IntoIterator for &PyDictRef {
    type Item = (PyObjectRef, PyObjectRef);
    type IntoIter = DictIter;

    fn into_iter(self) -> Self::IntoIter {
        DictIter::new(self.clone())
    }
}

pub struct DictIter {
    dict: PyDictRef,
    position: usize,
}

impl DictIter {
    pub fn new(dict: PyDictRef) -> DictIter {
        DictIter { dict, position: 0 }
    }
}

impl Iterator for DictIter {
    type Item = (PyObjectRef, PyObjectRef);

    fn next(&mut self) -> Option<Self::Item> {
        match self.dict.entries.next_entry(&mut self.position) {
            Some((key, value)) => Some((key, value)),
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let l = self.dict.entries.len_from_entry_index(self.position);
        (l, Some(l))
    }
}

macro_rules! dict_iterator {
    ( $name: ident, $iter_name: ident, $reverse_iter_name: ident,
      $class: ident, $iter_class: ident, $reverse_iter_class: ident,
      $class_name: literal, $iter_class_name: literal, $reverse_iter_class_name: literal,
      $result_fn: expr) => {
        #[pyclass(module=false,name = $class_name)]
        #[derive(Debug)]
        pub(crate) struct $name {
            pub dict: PyDictRef,
        }

        #[pyimpl(with(Comparable))]
        impl $name {
            fn new(dict: PyDictRef) -> Self {
                $name { dict }
            }

            #[pymethod(name = "__iter__")]
            fn iter(&self) -> $iter_name {
                $iter_name::new(self.dict.clone())
            }

            #[pymethod(name = "__len__")]
            fn len(&self) -> usize {
                self.dict.clone().len()
            }

            #[pymethod(name = "__repr__")]
            #[allow(clippy::redundant_closure_call)]
            fn repr(zelf: PyRef<Self>, vm: &VirtualMachine) -> PyResult<String> {
                let s = if let Some(_guard) = ReprGuard::enter(vm, zelf.as_object()) {
                    let mut str_parts = vec![];
                    for (key, value) in zelf.dict.clone() {
                        let s = vm.to_repr(&$result_fn(vm, key, value))?;
                        str_parts.push(s.borrow_value().to_owned());
                    }
                    format!("{}([{}])", $class_name, str_parts.join(", "))
                } else {
                    "{...}".to_owned()
                };
                Ok(s)
            }
            #[pymethod(name = "__reversed__")]
            fn reversed(&self) -> $reverse_iter_name {
                $reverse_iter_name::new(self.dict.clone())
            }
        }

        impl Comparable for $name {
            fn cmp(
                zelf: PyRef<Self>,
                other: PyObjectRef,
                op: PyComparisonOp,
                vm: &VirtualMachine,
            ) -> PyResult<PyComparisonValue> {
                match_class!(match other {
                    dictview @ Self => {
                        PyDict::inner_cmp(
                            zelf.dict.clone(),
                            dictview.dict.clone(),
                            op,
                            !zelf.lease_class().is(&vm.ctx.types.dict_keys_type),
                            vm,
                        )
                    }
                    _set @ PySet => {
                        // TODO: Implement comparison for set
                        Ok(NotImplemented)
                    }
                    _ => {
                        Ok(NotImplemented)
                    }
                })
            }
        }

        impl PyValue for $name {
            fn class(vm: &VirtualMachine) -> PyClassRef {
                vm.ctx.types.$class.clone()
            }
        }

        #[pyclass(module=false,name = $iter_class_name)]
        #[derive(Debug)]
        pub(crate) struct $iter_name {
            pub dict: PyDictRef,
            pub size: dictdatatype::DictSize,
            pub position: AtomicCell<usize>,
        }

        #[pyimpl]
        impl $iter_name {
            fn new(dict: PyDictRef) -> Self {
                $iter_name {
                    position: AtomicCell::new(0),
                    size: dict.size(),
                    dict,
                }
            }

            #[pymethod(name = "__next__")]
            #[allow(clippy::redundant_closure_call)]
            fn next(&self, vm: &VirtualMachine) -> PyResult {
                if self.dict.entries.has_changed_size(&self.size) {
                    return Err(
                        vm.new_runtime_error("dictionary changed size during iteration".to_owned())
                    );
                }
                let mut position = self.position.load();
                match self.dict.entries.next_entry(&mut position) {
                    Some((key, value)) => {
                        self.position.store(position);
                        Ok($result_fn(vm, key, value))
                    }
                    None => Err(objiter::new_stop_iteration(vm)),
                }
            }

            #[pymethod(name = "__iter__")]
            fn iter(zelf: PyRef<Self>) -> PyRef<Self> {
                zelf
            }

            #[pymethod(name = "__length_hint__")]
            fn length_hint(&self) -> usize {
                self.dict.entries.len_from_entry_index(self.position.load())
            }
        }

        impl PyValue for $iter_name {
            fn class(vm: &VirtualMachine) -> PyClassRef {
                vm.ctx.types.$iter_class.clone()
            }
        }

        #[pyclass(module=false,name = $reverse_iter_class_name)]
        #[derive(Debug)]
        pub(crate) struct $reverse_iter_name {
            pub dict: PyDictRef,
            pub size: dictdatatype::DictSize,
            pub position: AtomicCell<usize>,
        }

        #[pyimpl]
        impl $reverse_iter_name {
            fn new(dict: PyDictRef) -> Self {
                $reverse_iter_name {
                    position: AtomicCell::new(1),
                    size: dict.size(),
                    dict,
                }
            }

            #[pymethod(name = "__next__")]
            #[allow(clippy::redundant_closure_call)]
            fn next(&self, vm: &VirtualMachine) -> PyResult {
                if self.dict.entries.has_changed_size(&self.size) {
                    return Err(
                        vm.new_runtime_error("dictionary changed size during iteration".to_owned())
                    );
                }
                let count = self.position.fetch_add(1);
                match self.dict.len().checked_sub(count) {
                    Some(mut pos) => {
                        let (key, value) = self.dict.entries.next_entry(&mut pos).unwrap();
                        Ok($result_fn(vm, key, value))
                    }
                    None => {
                        self.position.store(std::isize::MAX as usize);
                        Err(objiter::new_stop_iteration(vm))
                    }
                }
            }

            #[pymethod(name = "__iter__")]
            fn iter(zelf: PyRef<Self>) -> PyRef<Self> {
                zelf
            }

            #[pymethod(name = "__length_hint__")]
            fn length_hint(&self) -> usize {
                self.dict.entries.len_from_entry_index(self.position.load())
            }
        }

        impl PyValue for $reverse_iter_name {
            fn class(vm: &VirtualMachine) -> PyClassRef {
                vm.ctx.types.$reverse_iter_class.clone()
            }
        }
    };
}

dict_iterator! {
    PyDictKeys,
    PyDictKeyIterator,
    PyDictReverseKeyIterator,
    dict_keys_type,
    dict_keyiterator_type,
    dict_reversekeyiterator_type,
    "dict_keys",
    "dict_keyiterator",
    "dict_reversekeyiterator",
    |_vm: &VirtualMachine, key: PyObjectRef, _value: PyObjectRef| key
}

dict_iterator! {
    PyDictValues,
    PyDictValueIterator,
    PyDictReverseValueIterator,
    dict_values_type,
    dict_valueiterator_type,
    dict_reversevalueiterator_type,
    "dict_values",
    "dict_valueiterator",
    "dict_reversevalueiterator",
    |_vm: &VirtualMachine, _key: PyObjectRef, value: PyObjectRef| value
}

dict_iterator! {
    PyDictItems,
    PyDictItemIterator,
    PyDictReverseItemIterator,
    dict_items_type,
    dict_itemiterator_type,
    dict_reverseitemiterator_type,
    "dict_items",
    "dict_itemiterator",
    "dict_reverseitemiterator",
    |vm: &VirtualMachine, key: PyObjectRef, value: PyObjectRef|
        vm.ctx.new_tuple(vec![key, value])
}

pub(crate) fn init(context: &PyContext) {
    PyDict::extend_class(context, &context.types.dict_type);
    PyDictKeys::extend_class(context, &context.types.dict_keys_type);
    PyDictKeyIterator::extend_class(context, &context.types.dict_keyiterator_type);
    PyDictReverseKeyIterator::extend_class(context, &context.types.dict_reversekeyiterator_type);
    PyDictValues::extend_class(context, &context.types.dict_values_type);
    PyDictValueIterator::extend_class(context, &context.types.dict_valueiterator_type);
    PyDictReverseValueIterator::extend_class(
        context,
        &context.types.dict_reversevalueiterator_type,
    );
    PyDictItems::extend_class(context, &context.types.dict_items_type);
    PyDictItemIterator::extend_class(context, &context.types.dict_itemiterator_type);
    PyDictReverseItemIterator::extend_class(context, &context.types.dict_reverseitemiterator_type);
}

pub struct PyMapping {
    dict: PyDictRef,
}

impl TryFromObject for PyMapping {
    fn try_from_object(vm: &VirtualMachine, obj: PyObjectRef) -> PyResult<Self> {
        let dict = vm.ctx.new_dict();
        PyDict::merge(
            &dict.entries,
            OptionalArg::Present(obj),
            KwArgs::default(),
            vm,
        )?;
        Ok(PyMapping { dict })
    }
}

impl PyMapping {
    pub fn into_dict(self) -> PyDictRef {
        self.dict
    }
}
