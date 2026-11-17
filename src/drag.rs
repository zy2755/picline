//! OLE 拖出：把相片文件作为 CF_HDROP 交给系统拖放。
//!
//! 等价 Electron 的 `webContents.startDrag({files:[it.file], icon})`：
//! 拖进应用 = 发送一份副本；拖进文件夹 = 保存一份；相片本身留在绳上。
//!
//! 手写最小 IDataObject / IDropSource vtable（不引入 windows-rs）；
//! DoDragDrop 会阻塞直到鼠标释放，期间由 Windows 泵送消息，
//! 因此 gui::tick 在 DRAGGING 期间整体跳过，避免重入窗口状态。

#![allow(dead_code)]

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub static DRAGGING: AtomicBool = AtomicBool::new(false);

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const DV_E_FORMATETC: i32 = 0x8004_0064u32 as i32;
const DRAGDROP_S_DROP: i32 = 0x0004_0100;
const DRAGDROP_S_CANCEL: i32 = 0x0004_0101;
const DRAGDROP_S_USEDEFAULTCURSORS: i32 = 0x0004_0102;
const CF_HDROP: u16 = 15;
const TYMED_HGLOBAL: u32 = 1;
const DROPEFFECT_COPY: u32 = 1;
const DATADIR_GET: u32 = 1;
const DVASPECT_CONTENT: u32 = 1;
const DATA_S_SAMEFORMATETC: i32 = 0x0004_0130;
/// {00000103-0000-0000-C000-000000000046}
const IID_IENUMFORMATETC: Guid = Guid { d1: 0x103, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
const MK_LBUTTON: u32 = 1;
const GMEM_MOVEABLE: u32 = 0x0002;
const GMEM_ZEROINIT: u32 = 0x0040;

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}
const IID_UNKNOWN: Guid = Guid { d1: 0, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
const IID_IDATAOBJECT: Guid = Guid { d1: 0x10E, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
/// {00000121-0000-0000-C000-000000000046} —— DoDragDrop 会 QI 校验 IDropSource
const IID_IDROPSOURCE: Guid = Guid { d1: 0x121, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };

#[repr(C)]
#[derive(Clone, Copy)]
struct FormatEtc {
    cf_format: u16,
    ptd: *mut c_void,
    dw_aspect: u32,
    lindex: i32,
    tymed: u32,
}
#[repr(C)]
struct StgMedium {
    tymed: u32,
    _pad: u32,
    h_global: isize,
    p_unk: *mut c_void,
}
#[repr(C)]
struct DropFiles {
    p_files: u32,
    pt: [i32; 2],
    f_nc: i32,
    f_wide: i32,
}

#[repr(C)]
struct DataObject {
    vtbl: *const DataObjectVtbl,
    refs: u32,
    /// 以双 NUL 结尾的 UTF-16 绝对路径
    name: Vec<u16>,
}
#[repr(C)]
struct DataObjectVtbl {
    query_interface: extern "system" fn(*mut DataObject, *const Guid, *mut *mut c_void) -> i32,
    add_ref: extern "system" fn(*mut DataObject) -> u32,
    release: extern "system" fn(*mut DataObject) -> u32,
    get_data: extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> i32,
    get_data_here: extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> i32,
    query_get_data: extern "system" fn(*mut DataObject, *const FormatEtc) -> i32,
    get_canonical_format_etc: extern "system" fn(*mut DataObject, *const FormatEtc, *mut FormatEtc) -> i32,
    set_data: extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium, i32) -> i32,
    enum_format_etc: extern "system" fn(*mut DataObject, u32, *mut *mut c_void) -> i32,
    d_advise: extern "system" fn(*mut DataObject, *const FormatEtc, u32, *mut c_void, *mut u32) -> i32,
    d_unadvise: extern "system" fn(*mut DataObject, u32) -> i32,
    enum_d_advise: extern "system" fn(*mut DataObject, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct DropSource {
    vtbl: *const DropSourceVtbl,
    refs: u32,
}
#[repr(C)]
struct DropSourceVtbl {
    query_interface: extern "system" fn(*mut DropSource, *const Guid, *mut *mut c_void) -> i32,
    add_ref: extern "system" fn(*mut DropSource) -> u32,
    release: extern "system" fn(*mut DropSource) -> u32,
    query_continue_drag: extern "system" fn(*mut DropSource, i32, u32) -> i32,
    give_feedback: extern "system" fn(*mut DropSource, u32) -> i32,
}

fn same_guid(a: &Guid, b: &Guid) -> bool {
    a.d1 == b.d1 && a.d2 == b.d2 && a.d3 == b.d3 && a.d4 == b.d4
}

extern "system" fn do_qi(this: *mut DataObject, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    unsafe {
        let i = &*riid;
        if same_guid(i, &IID_UNKNOWN) || same_guid(i, &IID_IDATAOBJECT) {
            *ppv = this as *mut c_void;
            (*this).refs += 1;
            S_OK
        } else {
            *ppv = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}
extern "system" fn do_add_ref(this: *mut DataObject) -> u32 {
    unsafe {
        (*this).refs += 1;
        (*this).refs
    }
}
extern "system" fn do_release(this: *mut DataObject) -> u32 {
    unsafe {
        (*this).refs -= 1;
        let n = (*this).refs;
        if n == 0 {
            drop(Box::from_raw(this));
        }
        n
    }
}
extern "system" fn do_get_data(this: *mut DataObject, fmt: *const FormatEtc, med: *mut StgMedium) -> i32 {
    if fmt.is_null() || med.is_null() {
        return E_POINTER;
    }
    unsafe {
        let f = &*fmt;
        if f.cf_format != CF_HDROP || (f.tymed & TYMED_HGLOBAL) == 0 {
            return DV_E_FORMATETC;
        }
        let name = &(*this).name;
        let size = std::mem::size_of::<DropFiles>() + name.len() * 2;
        let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, size);
        if h == 0 {
            return E_NOTIMPL;
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            return E_NOTIMPL;
        }
        *(p as *mut DropFiles) = DropFiles {
            p_files: std::mem::size_of::<DropFiles>() as u32,
            pt: [0, 0],
            f_nc: 0,
            f_wide: 1,
        };
        let base = std::mem::size_of::<DropFiles>();
        for (i, c) in name.iter().enumerate() {
            std::ptr::write_unaligned(p.add(base + i * 2) as *mut u16, *c);
        }
        GlobalUnlock(h);
        *med = StgMedium {
            tymed: TYMED_HGLOBAL,
            _pad: 0,
            h_global: h,
            p_unk: std::ptr::null_mut(),
        };
        S_OK
    }
}
/// Explorer 有时改用 GetDataHere 取 CF_HDROP；复用 GetData 的构造即可。
extern "system" fn do_get_data_here(t: *mut DataObject, f: *const FormatEtc, m: *mut StgMedium) -> i32 {
    do_get_data(t, f, m)
}
extern "system" fn do_query_get_data(_t: *mut DataObject, f: *const FormatEtc) -> i32 {
    unsafe {
        if !f.is_null() && (*f).cf_format == CF_HDROP && ((*f).tymed & TYMED_HGLOBAL) != 0 {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }
}
extern "system" fn do_canonical(_t: *mut DataObject, f: *const FormatEtc, o: *mut FormatEtc) -> i32 {
    if f.is_null() || o.is_null() {
        return E_POINTER;
    }
    unsafe {
        *o = *f;
        (*o).ptd = std::ptr::null_mut();
    }
    DATA_S_SAMEFORMATETC
}
extern "system" fn do_set_data(_t: *mut DataObject, _f: *const FormatEtc, _m: *mut StgMedium, _r: i32) -> i32 { E_NOTIMPL }
/// 必须实现：Explorer 在 DragEnter 时用 EnumFormatEtc(DATADIR_GET) 枚举可用格式，
/// 返回 E_NOTIMPL 会让它直接判定"无可用数据"，DoDragDrop 最终 effect=0、不产生副本。
extern "system" fn do_enum_fmt(_t: *mut DataObject, d: u32, o: *mut *mut c_void) -> i32 {
    if o.is_null() {
        return E_POINTER;
    }
    if d != DATADIR_GET {
        return E_NOTIMPL;
    }
    unsafe {
        let e = Box::into_raw(Box::new(EnumFmt { vtbl: &EF_VTBL, refs: 1, index: 0 }));
        *o = e as *mut c_void;
    }
    S_OK
}
extern "system" fn do_dadvise(_t: *mut DataObject, _f: *const FormatEtc, _a: u32, _s: *mut c_void, _c: *mut u32) -> i32 { E_NOTIMPL }
extern "system" fn do_dunadvise(_t: *mut DataObject, _c: u32) -> i32 { E_NOTIMPL }
extern "system" fn do_enum_dadvise(_t: *mut DataObject, _o: *mut *mut c_void) -> i32 { E_NOTIMPL }

static DO_VTBL: DataObjectVtbl = DataObjectVtbl {
    query_interface: do_qi,
    add_ref: do_add_ref,
    release: do_release,
    get_data: do_get_data,
    get_data_here: do_get_data_here,
    query_get_data: do_query_get_data,
    get_canonical_format_etc: do_canonical,
    set_data: do_set_data,
    enum_format_etc: do_enum_fmt,
    d_advise: do_dadvise,
    d_unadvise: do_dunadvise,
    enum_d_advise: do_enum_dadvise,
};

extern "system" fn ds_qi(this: *mut DropSource, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    unsafe {
        if same_guid(&*riid, &IID_UNKNOWN) || same_guid(&*riid, &IID_IDROPSOURCE) {
            *ppv = this as *mut c_void;
            (*this).refs += 1;
            S_OK
        } else {
            *ppv = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}
extern "system" fn ds_add_ref(this: *mut DropSource) -> u32 {
    unsafe { (*this).refs += 1; (*this).refs }
}
extern "system" fn ds_release(this: *mut DropSource) -> u32 {
    unsafe {
        (*this).refs -= 1;
        let n = (*this).refs;
        if n == 0 {
            drop(Box::from_raw(this));
        }
        n
    }
}
extern "system" fn ds_query_continue(_t: *mut DropSource, esc: i32, keys: u32) -> i32 {
    if esc != 0 {
        DRAGDROP_S_CANCEL
    } else if keys & MK_LBUTTON == 0 {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}
extern "system" fn ds_give_feedback(_t: *mut DropSource, _e: u32) -> i32 {
    DRAGDROP_S_USEDEFAULTCURSORS
}
static DS_VTBL: DropSourceVtbl = DropSourceVtbl {
    query_interface: ds_qi,
    add_ref: ds_add_ref,
    release: ds_release,
    query_continue_drag: ds_query_continue,
    give_feedback: ds_give_feedback,
};

// ---------------- 最小 IEnumFORMATETC ----------------
// 只枚举一种格式：CF_HDROP / TYMED_HGLOBAL。Shell 的 drop target 依赖它。
#[repr(C)]
struct EnumFmt {
    vtbl: *const EnumFmtVtbl,
    refs: u32,
    index: u32,
}
#[repr(C)]
struct EnumFmtVtbl {
    query_interface: extern "system" fn(*mut EnumFmt, *const Guid, *mut *mut c_void) -> i32,
    add_ref: extern "system" fn(*mut EnumFmt) -> u32,
    release: extern "system" fn(*mut EnumFmt) -> u32,
    next: extern "system" fn(*mut EnumFmt, u32, *mut FormatEtc, *mut u32) -> i32,
    skip: extern "system" fn(*mut EnumFmt, u32) -> i32,
    reset: extern "system" fn(*mut EnumFmt) -> i32,
    clone: extern "system" fn(*mut EnumFmt, *mut *mut c_void) -> i32,
}
extern "system" fn ef_qi(this: *mut EnumFmt, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    unsafe {
        let i = &*riid;
        if same_guid(i, &IID_UNKNOWN) || same_guid(i, &IID_IENUMFORMATETC) {
            *ppv = this as *mut c_void;
            (*this).refs += 1;
            S_OK
        } else {
            *ppv = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}
extern "system" fn ef_add_ref(this: *mut EnumFmt) -> u32 {
    unsafe { (*this).refs += 1; (*this).refs }
}
extern "system" fn ef_release(this: *mut EnumFmt) -> u32 {
    unsafe {
        (*this).refs -= 1;
        let n = (*this).refs;
        if n == 0 {
            drop(Box::from_raw(this));
        }
        n
    }
}
extern "system" fn ef_next(this: *mut EnumFmt, celt: u32, rgelt: *mut FormatEtc, fetched: *mut u32) -> i32 {
    if rgelt.is_null() {
        return E_POINTER;
    }
    unsafe {
        let e = &mut *this;
        let mut n = 0u32;
        while n < celt && e.index == 0 {
            *rgelt.add(n as usize) = FormatEtc {
                cf_format: CF_HDROP,
                ptd: std::ptr::null_mut(),
                dw_aspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            };
            e.index = 1;
            n += 1;
        }
        if !fetched.is_null() {
            *fetched = n;
        }
        if n == celt { S_OK } else { S_FALSE }
    }
}
extern "system" fn ef_skip(this: *mut EnumFmt, celt: u32) -> i32 {
    unsafe { (*this).index = (*this).index.saturating_add(celt); }
    S_OK
}
extern "system" fn ef_reset(this: *mut EnumFmt) -> i32 {
    unsafe { (*this).index = 0; }
    S_OK
}
extern "system" fn ef_clone(_t: *mut EnumFmt, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { *out = std::ptr::null_mut(); }
    E_NOTIMPL
}
static EF_VTBL: EnumFmtVtbl = EnumFmtVtbl {
    query_interface: ef_qi,
    add_ref: ef_add_ref,
    release: ef_release,
    next: ef_next,
    skip: ef_skip,
    reset: ef_reset,
    clone: ef_clone,
};

#[link(name = "ole32")]
extern "system" {
    /// MSDN: DoDragDrop 之前必须 OleInitialize（它会额外注册 OLE 拖放所需的窗口类），
    /// 只调 CoInitialize 会让 DoDragDrop 返回失败码。
    fn OleInitialize(p: *mut c_void) -> i32;
    fn OleUninitialize();
    fn DoDragDrop(obj: *mut c_void, src: *mut c_void, ok: u32, effect: *mut u32) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, size: usize) -> isize;
    fn GlobalLock(h: isize) -> *mut c_void;
    fn GlobalUnlock(h: isize) -> i32;
}

fn wide_double(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v.push(0);
    v
}

/// 执行一次系统拖放（阻塞到鼠标释放）。返回是否成功启动并完成。
pub fn drag_file(path: &Path) -> bool {
    DRAGGING.store(true, Ordering::SeqCst);
    let co = unsafe { OleInitialize(std::ptr::null_mut()) };
    let ok: bool;
    unsafe {
        let obj = Box::into_raw(Box::new(DataObject {
            vtbl: &DO_VTBL,
            refs: 1,
            name: wide_double(&path.to_string_lossy()),
        }));
        let src = Box::into_raw(Box::new(DropSource { vtbl: &DS_VTBL, refs: 1 }));
        let mut effect: u32 = 0;
        let hr = DoDragDrop(obj as *mut c_void, src as *mut c_void, DROPEFFECT_COPY, &mut effect);
        // effect=0 (DROPEFFECT_NONE) 表示目标没有接受数据；即使 hr 是
        // DRAGDROP_S_DROP（来自 QueryContinueDrag）也不能算拖出成功。
        ok = hr >= 0 && (effect & DROPEFFECT_COPY) != 0;
        crate::trace::ev("drag_hresult", &format!("hr=0x{:08X} effect={} oleinit=0x{:08X} ok={}", hr as u32, effect, co as u32, ok));
        // 释放我们持有的初始引用（DoDragDrop 按 COM 规则自行 AddRef/Release）
        (DO_VTBL.release)(obj);
        (DS_VTBL.release)(src);
        if co == S_OK || co == S_FALSE {
            OleUninitialize();
        }
    }
    DRAGGING.store(false, Ordering::SeqCst);
    ok
}

/// 不进入 DoDragDrop 的自检：验证 vtable / 引用计数 / CF_HDROP 数据构造。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_format_etc_reports_hdrop_exactly_once() {
        let obj = Box::into_raw(Box::new(DataObject {
            vtbl: &DO_VTBL,
            refs: 1,
            name: wide_double("C:\\inbox\\shot.png"),
        }));
        unsafe {
            let mut out: *mut c_void = std::ptr::null_mut();
            assert_eq!((DO_VTBL.enum_format_etc)(obj, DATADIR_GET, &mut out), S_OK);
            assert!(!out.is_null());
            let e = out as *mut EnumFmt;
            let mut fmt: FormatEtc = std::mem::zeroed();
            let mut fetched = 0u32;
            assert_eq!((EF_VTBL.next)(e, 1, &mut fmt, &mut fetched), S_OK);
            assert_eq!(fmt.cf_format, CF_HDROP);
            assert_eq!(fmt.tymed, TYMED_HGLOBAL);
            assert_eq!(fetched, 1);
            assert_eq!((EF_VTBL.next)(e, 1, &mut fmt, &mut fetched), S_FALSE);
            assert_eq!((EF_VTBL.reset)(e), S_OK);
            assert_eq!((EF_VTBL.next)(e, 1, &mut fmt, &mut fetched), S_OK);
            let mut q: *mut c_void = std::ptr::null_mut();
            assert_eq!((EF_VTBL.query_interface)(e, &IID_IENUMFORMATETC, &mut q), S_OK);
            assert_eq!((EF_VTBL.release)(e), 1);
            assert_eq!((EF_VTBL.release)(e), 0);
            (DO_VTBL.release)(obj);
        }
    }

    #[test]
    fn data_object_refcount_and_hdrop() {
        let obj = Box::into_raw(Box::new(DataObject {
            vtbl: &DO_VTBL,
            refs: 1,
            name: wide_double("C:\\inbox\\shot.png"),
        }));
        unsafe {
            assert_eq!((DO_VTBL.add_ref)(obj), 2);
            let mut out: *mut c_void = std::ptr::null_mut();
            assert_eq!((DO_VTBL.query_interface)(obj, &IID_UNKNOWN, &mut out), S_OK);
            assert_eq!(out, obj as *mut c_void);
            let fmt = FormatEtc {
                cf_format: CF_HDROP,
                ptd: std::ptr::null_mut(),
                dw_aspect: 1,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            };
            let mut med: StgMedium = std::mem::zeroed();
            assert_eq!((DO_VTBL.get_data)(obj, &fmt, &mut med), S_OK);
            assert_eq!(med.tymed, TYMED_HGLOBAL);
            assert_ne!(med.h_global, 0);
            // 校验 DROPFILES 头
            let p = GlobalLock(med.h_global) as *const DropFiles;
            assert_eq!((*p).p_files, 20);
            assert_eq!((*p).f_wide, 1);
            GlobalUnlock(med.h_global);
            crate::api::GlobalFree(med.h_global);
            assert_eq!((DO_VTBL.release)(obj), 2);
            assert_eq!((DO_VTBL.release)(obj), 1);
            assert_eq!((DO_VTBL.release)(obj), 0);
        }
    }

    #[test]
    fn drop_source_qi_answers_idropsource() {
        let s = Box::into_raw(Box::new(DropSource { vtbl: &DS_VTBL, refs: 1 }));
        let mut out: *mut c_void = std::ptr::null_mut();
        let hr = ds_qi(s, &IID_IDROPSOURCE, &mut out);
        assert_eq!(hr, S_OK);
        assert_eq!(out, s as *mut c_void);
        assert_eq!(IID_IDROPSOURCE.d1, 0x121);
        assert_eq!((DS_VTBL.release)(s), 1); // QI 增加的一次引用
        assert_eq!((DS_VTBL.release)(s), 0);
    }

    #[test]
    fn query_continue_drag_state_machine() {
        let s = Box::into_raw(Box::new(DropSource { vtbl: &DS_VTBL, refs: 1 }));
        assert_eq!(ds_query_continue(s, 1, MK_LBUTTON), DRAGDROP_S_CANCEL);
        assert_eq!(ds_query_continue(s, 0, 0), DRAGDROP_S_DROP);
        assert_eq!(ds_query_continue(s, 0, MK_LBUTTON), S_OK);
        assert_eq!((DS_VTBL.release)(s), 0);
    }
}
