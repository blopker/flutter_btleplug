use jni::objects::GlobalRef;
use jni::{AttachGuard, JNIEnv, JavaVM};
use once_cell::sync::OnceCell;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ble::Error;
use crate::logger::log;

use super::RUNTIME;

static CLASS_LOADER: OnceCell<GlobalRef> = OnceCell::new();
pub static JAVAVM: OnceCell<JavaVM> = OnceCell::new();

std::thread_local! {
    static JNI_ENV: RefCell<Option<AttachGuard<'static>>> = RefCell::new(None);
}

pub fn create_runtime() -> Result<(), Error> {
    log("CREATE RUNTIME".to_owned());
    let vm = JAVAVM.get().ok_or(Error::JavaVM)?;
    let env = vm.attach_current_thread().unwrap();

    setup_class_loader(&env)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("intiface-thread-{}", id)
        })
        .on_thread_stop(move || {
            log("STOPPING THREAD");
            JNI_ENV.with(|f| *f.borrow_mut() = None);
        })
        .on_thread_start(move || {
            log("WRAPPING NEW THREAD IN VM");

            // We now need to call the following code block via JNI calls. God help us.
            //
            //  java.lang.Thread.currentThread().setContextClassLoader(
            //    java.lang.ClassLoader.getSystemClassLoader()
            //  );
            log("Adding classloader to thread");

            let vm = JAVAVM.get().unwrap();
            let env = vm.attach_current_thread().unwrap();

            let thread = env
                .call_static_method(
                    "java/lang/Thread",
                    "currentThread",
                    "()Ljava/lang/Thread;",
                    &[],
                )
                .unwrap()
                .l()
                .unwrap();
            env.call_method(
                thread,
                "setContextClassLoader",
                "(Ljava/lang/ClassLoader;)V",
                &[CLASS_LOADER.get().unwrap().as_obj().into()],
            )
            .unwrap();
            log("Classloader added to thread");
            JNI_ENV.with(|f| *f.borrow_mut() = Some(env));
        })
        .build()
        .unwrap();
    RUNTIME.set(runtime).map_err(|_| Error::Runtime)?;
    Ok(())
}

fn setup_class_loader(env: &JNIEnv) -> Result<(), Error> {
    let thread = env
        .call_static_method(
            "java/lang/Thread",
            "currentThread",
            "()Ljava/lang/Thread;",
            &[],
        )?
        .l()?;
    let class_loader = env
        .call_method(
            thread,
            "getContextClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;

    CLASS_LOADER
        .set(env.new_global_ref(class_loader)?)
        .map_err(|_| Error::ClassLoader)
}

#[no_mangle]
pub extern "C" fn JNI_OnLoad(vm: jni::JavaVM, res: *const std::os::raw::c_void) -> jni::sys::jint {
    let _res = res;
    let env = vm.get_env().unwrap();
    jni_utils::init(&env).unwrap();
    btleplug::platform::init(&env).unwrap();
    let _ = JAVAVM.set(vm);
    jni::JNIVersion::V6.into()
}
