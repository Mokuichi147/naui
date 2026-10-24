//! The foundation for inheriting composable WinRT classes on the Rust side.
//!
//! What can be passed to `Application::Start` is not `Application` itself but a
//! **composed object** implementing `IApplicationOverrides::OnLaunched`. In WinRT,
//! it is created through COM aggregation.
//!
//! There are three steps:
//!
//! 1. Get a **composition factory** such as `IApplicationFactory` from the base
//!    class's activation factory
//! 2. Call `CreateInstance(outer, &mut inner, &mut result)`. `outer` is our COM
//!    object, `inner` the base class's "non-delegating IUnknown", and `result` the
//!    resulting base class handle
//! 3. `outer`'s `QueryInterface` checks what it implements itself first, and passes
//!    unknown IIDs on to `inner`
//!
//! [`Compose`] wraps these three. Types that want to inherit implement
//! [`ChildClass`].

use std::ffi::c_void;
use std::ptr::NonNull;

use windows_core::imp::WeakRefCount;
use windows_core::{
    factory, ComObject, ComObjectInner, ComObjectInterface, IInspectable, IInspectable_Vtbl,
    IUnknownImpl, Interface, InterfaceRef, Result, RuntimeName, Type, TypeKind, GUID, HRESULT,
};

/// The ABI of `IXxxFactory::CreateInstance`.
///
/// Every composition factory has this shape (when the base class has constructor
/// arguments they are added in front, but `Application`, which naui inherits, has
/// none).
pub type CreateInstanceFn = unsafe extern "system" fn(
    this: *mut c_void,
    outer: *mut c_void,
    inner: *mut *mut c_void,
    result: *mut *mut c_void,
) -> HRESULT;

/// The inheriting type.
///
/// The implementing type is also a `windows_core` COM object ([`ComObjectInner`]).
/// [`Compose`] borrows the vtable layout and reference count placed in `Outer`, so
/// functions to take those two out are required.
pub trait ChildClass: ComObjectInner {
    /// The base class being inherited (`Application` and so on).
    type BaseType: RuntimeName + TypeKind + Type<Self::BaseType, Abi = *mut c_void>;
    /// The base class's composition factory (`IApplicationFactory` and so on).
    type FactoryInterface: Interface;

    /// Takes the `CreateInstance` slot out of the factory's vtable.
    fn create_interface_fn(
        vtable: &<Self::FactoryInterface as Interface>::Vtable,
    ) -> CreateInstanceFn;

    /// A reference to the identity vtable placed at the head of `Outer`.
    ///
    /// [`Compose`] replaces this with its own vtable to intercept `QueryInterface`.
    fn identity_vtable(outer: &mut Self::Outer) -> &mut &'static IInspectable_Vtbl;

    /// The reference count held by `Outer`.
    fn ref_count(outer: &Self::Outer) -> &WeakRefCount;

    /// Turns it into COM object form.
    fn into_outer(self) -> Self::Outer;
}

/// Composes the inheriting type with the base class.
#[repr(transparent)]
pub struct Compose<T> {
    child: T,
}

impl<T: ChildClass> Compose<T>
where
    T::Outer: ComObjectInterface<IInspectable>,
{
    /// Composes `child` with the base class and returns the base class handle.
    ///
    /// The returned handle points at the whole composed object. `child`'s lifetime is
    /// held by the COM reference count, so the caller need not keep it separately.
    pub fn compose(child: T) -> Result<T::BaseType> {
        Self::compose_with(child, &factory::<T::BaseType, T::FactoryInterface>()?)
    }

    /// Composes with a factory prepared by the caller.
    pub fn compose_with(child: T, factory: &T::FactoryInterface) -> Result<T::BaseType> {
        // `IInspectable` へ変換した時点で Box され、参照数 1 で生きる。
        let outer: IInspectable = Self { child }.into();
        unsafe {
            let outer_raw = outer.as_raw();
            // `Composed<T>` は `#[repr(C)]` で先頭が `T::Outer` なので、
            // 識別 vtable のアドレス = オブジェクトの先頭になる。
            let composed = outer_raw as *mut Composed<T>;
            let inner = &mut (*composed).inner;
            let mut result = std::ptr::null_mut();
            T::create_interface_fn(factory.vtable())(
                factory.as_raw(),
                outer_raw,
                inner as *mut _ as *mut *mut c_void,
                &mut result,
            )
            .and_then(|| Type::from_abi(result))
        }
    }
}

/// The composed object itself.
///
/// The inheriting side's COM object is placed at the head, followed by the base
/// class's "non-delegating IUnknown". The address of the first half is the object's
/// address, so from COM's point of view it looks like the inheriting side itself.
#[repr(C)]
#[doc(hidden)]
pub struct Composed<T: ComObjectInner> {
    child: T::Outer,
    inner: Option<IInspectable>,
}

impl<T: ChildClass> Composed<T> {
    const VTABLE_IDENTITY: IInspectable_Vtbl =
        IInspectable_Vtbl::new::<Composed<T>, T::BaseType, 0>();
}

impl<T: ChildClass> Compose<T> {
    fn into_outer(self) -> Composed<T> {
        let mut child = self.child.into_outer();
        // 識別 vtable を差し替えて、IUnknown / IInspectable の呼び出しが
        // `Composed` 側へ来るようにする。基底クラスへの委譲はここでしか
        // 挟めない。
        *T::identity_vtable(&mut child) = &Composed::<T>::VTABLE_IDENTITY;
        Composed { child, inner: None }
    }
}

impl<T: ChildClass> IUnknownImpl for Composed<T> {
    type Impl = Compose<T>;

    fn get_impl(&self) -> &Self::Impl {
        // SAFETY: `Compose<T>` は `T` の `#[repr(transparent)]` な包み。
        unsafe { &*(self.child.get_impl() as *const T as *const Compose<T>) }
    }

    fn get_impl_mut(&mut self) -> &mut Self::Impl {
        // SAFETY: `get_impl` と同じ。
        unsafe { &mut *(self.child.get_impl_mut() as *mut T as *mut Compose<T>) }
    }

    fn into_inner(self) -> Self::Impl {
        Compose {
            child: self.child.into_inner(),
        }
    }

    unsafe fn QueryInterface(&self, iid: *const GUID, interface: *mut *mut c_void) -> HRESULT {
        // 自分が実装している分が先。知らない IID だけ基底クラスへ回す。
        let found = unsafe { self.child.QueryInterface(iid, interface) };
        if found == windows::Win32::Foundation::E_NOINTERFACE {
            if let Some(inner) = &self.inner {
                return unsafe { inner.query(iid, interface) };
            }
        }
        found
    }

    fn AddRef(&self) -> u32 {
        T::ref_count(&self.child).add_ref()
    }

    unsafe fn Release(self_: *mut Self) -> u32 {
        let remaining = unsafe { T::ref_count(&(*self_).child).release() };
        if remaining == 0 {
            // SAFETY: `into_object` が Box で確保している。
            drop(unsafe { Box::from_raw(self_) });
        }
        remaining
    }

    fn is_reference_count_one(&self) -> bool {
        self.child.is_reference_count_one()
    }

    unsafe fn GetTrustLevel(&self, value: *mut i32) -> HRESULT {
        unsafe { self.child.GetTrustLevel(value) }
    }

    fn to_object(&self) -> ComObject<Self::Impl> {
        self.AddRef();
        // SAFETY: 参照数を 1 つ足したうえで、生きているオブジェクトを渡す。
        unsafe { ComObject::from_raw(NonNull::from(self)) }
    }
}

impl<T: ChildClass> ComObjectInner for Compose<T> {
    type Outer = Composed<T>;

    fn into_object(self) -> ComObject<Self> {
        let boxed = Box::new(self.into_outer());
        // SAFETY: Box から取った生ポインタは null にならない。
        unsafe { ComObject::from_raw(NonNull::new_unchecked(Box::into_raw(boxed))) }
    }
}

impl<T: ChildClass> ComObjectInterface<IInspectable> for Composed<T>
where
    T::Outer: ComObjectInterface<IInspectable>,
{
    fn as_interface_ref(&self) -> InterfaceRef<'_, IInspectable> {
        self.child.as_interface_ref()
    }
}

impl<T: ChildClass> From<Compose<T>> for IInspectable
where
    T::Outer: ComObjectInterface<IInspectable>,
{
    fn from(value: Compose<T>) -> Self {
        ComObject::new(value).into_interface()
    }
}
