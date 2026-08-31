//! composable な WinRT クラスを Rust 側で継承するための土台。
//!
//! `Application::Start` に渡せるのは `Application` そのものではなく、
//! `IApplicationOverrides::OnLaunched` を実装した**合成オブジェクト**
//! (composed object) である。WinRT では、これを COM の集約 (aggregation) で
//! 作る。
//!
//! 手順は 3 つ。
//!
//! 1. 基底クラスのアクティベーションファクトリから `IApplicationFactory` の
//!    ような **合成用ファクトリ**を得る
//! 2. `CreateInstance(outer, &mut inner, &mut result)` を呼ぶ。`outer` は
//!    こちらの COM オブジェクト、`inner` は基底クラスの「委譲しない
//!    IUnknown」、`result` は出来上がった基底クラスのハンドル
//! 3. `outer` の `QueryInterface` は、自分が実装している分を先に見て、
//!    知らない IID は `inner` へ回す
//!
//! [`Compose`] はこの 3 つを包む。継承したい型は [`ChildClass`] を実装する。

use std::ffi::c_void;
use std::ptr::NonNull;

use windows_core::imp::WeakRefCount;
use windows_core::{
    factory, ComObject, ComObjectInner, ComObjectInterface, IInspectable, IInspectable_Vtbl,
    IUnknownImpl, Interface, InterfaceRef, Result, RuntimeName, Type, TypeKind, GUID, HRESULT,
};

/// `IXxxFactory::CreateInstance` の ABI。
///
/// 合成用ファクトリはどれもこの形をしている (基底クラスにコンストラクタ
/// 引数があるときは前に足されるが、naui が継承する `Application` には無い)。
pub type CreateInstanceFn = unsafe extern "system" fn(
    this: *mut c_void,
    outer: *mut c_void,
    inner: *mut *mut c_void,
    result: *mut *mut c_void,
) -> HRESULT;

/// 継承する側の型。
///
/// 実装する型は `windows_core` の COM オブジェクト
/// ([`ComObjectInner`]) でもある。`Outer` に置いた vtable の並びと参照数を
/// [`Compose`] が借りるため、その 2 つを取り出す関数を要求する。
pub trait ChildClass: ComObjectInner {
    /// 継承する基底クラス (`Application` など)。
    type BaseType: RuntimeName + TypeKind + Type<Self::BaseType, Abi = *mut c_void>;
    /// 基底クラスの合成用ファクトリ (`IApplicationFactory` など)。
    type FactoryInterface: Interface;

    /// ファクトリの vtable から `CreateInstance` のスロットを取り出す。
    fn create_interface_fn(
        vtable: &<Self::FactoryInterface as Interface>::Vtable,
    ) -> CreateInstanceFn;

    /// `Outer` の先頭に置いた識別 vtable への参照。
    ///
    /// [`Compose`] は、ここを自分の vtable で差し替えて
    /// `QueryInterface` に割り込む。
    fn identity_vtable(outer: &mut Self::Outer) -> &mut &'static IInspectable_Vtbl;

    /// `Outer` が持つ参照数。
    fn ref_count(outer: &Self::Outer) -> &WeakRefCount;

    /// COM オブジェクトの形にする。
    fn into_outer(self) -> Self::Outer;
}

/// 継承した型を基底クラスと合成する。
#[repr(transparent)]
pub struct Compose<T> {
    child: T,
}

impl<T: ChildClass> Compose<T>
where
    T::Outer: ComObjectInterface<IInspectable>,
{
    /// `child` を基底クラスと合成し、基底クラスのハンドルを返す。
    ///
    /// 返ったハンドルは合成オブジェクト全体を指す。`child` の寿命は COM の
    /// 参照数が持つので、呼び出し側が別に保持する必要は無い。
    pub fn compose(child: T) -> Result<T::BaseType> {
        Self::compose_with(child, &factory::<T::BaseType, T::FactoryInterface>()?)
    }

    /// ファクトリを自分で用意して合成する。
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

/// 合成オブジェクトの実体。
///
/// 先頭に継承する側の COM オブジェクトを置き、そのうしろに基底クラスの
/// 「委譲しない IUnknown」を持つ。前半のアドレスがそのままオブジェクトの
/// アドレスになるので、COM から見ると継承する側そのものに見える。
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
