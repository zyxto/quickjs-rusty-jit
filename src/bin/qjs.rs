use libquickjs_ng_sys as q;
use quickjs_rusty::Context;
use std::env;
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{self, Read};

// Native, highly optimized logging function.
// Bypasses all Rust argument wrapping, JSON serialization, and intermediate vector copies.
// Directly converts JSValue to CString and prints to stdout.
unsafe extern "C" fn js_native_log(
    ctx: *mut q::JSContext,
    _this_val: q::JSValue,
    argc: i32,
    argv: *mut q::JSValue,
) -> q::JSValue {
    for i in 0..argc {
        let val = *argv.add(i as usize);
        let ptr = q::JS_ToCStringLen2(ctx, std::ptr::null_mut(), val, false);
        if !ptr.is_null() {
            let cstr = CStr::from_ptr(ptr);
            if i > 0 {
                print!(" ");
            }
            print!("{}", cstr.to_string_lossy());
            q::JS_FreeCString(ctx, ptr);
        }
    }
    println!();
    q::JSValue {
        u: q::JSValueUnion { int32: 0 },
        tag: 3, // JS_TAG_UNDEFINED is 3
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Check for --performance parameter
    let performance = args.iter().any(|arg| arg == "--performance");

    // Filter out binary name and --performance flag to find potential script path
    let filtered_args: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|arg| *arg != "--performance")
        .collect();

    let code = if filtered_args.is_empty() {
        // Read from stdin
        let mut buffer = String::new();
        let mut stdin = io::stdin();
        stdin
            .read_to_string(&mut buffer)
            .expect("Error reading from stdin");
        buffer
    } else {
        // Read from file path
        let path = filtered_args[0];
        fs::read_to_string(path).unwrap_or_else(|err| {
            eprintln!("Error reading file '{}': {}", path, err);
            std::process::exit(1);
        })
    };

    let context = Context::new(None).expect("Failed to create JS Context");

    // Register our high-performance native log function
    unsafe {
        let ctx_ptr = context.raw_context();
        let name_cstr = CString::new("__log").unwrap();
        let func_val = q::JS_NewCFunction2(
            ctx_ptr,
            Some(js_native_log),
            name_cstr.as_ptr(),
            0,
            0, // JS_CFUNC_generic
            0,
        );
        let global_val = q::JS_GetGlobalObject(ctx_ptr);
        q::JS_SetPropertyStr(ctx_ptr, global_val, name_cstr.as_ptr(), func_val);
        q::JS_FreeValue(ctx_ptr, global_val);
    }

    // Embed and load the console.js module
    let console_js = include_str!("../../modules/console.js");
    context
        .eval(console_js, false)
        .expect("Failed to load console.js module");

    // Evaluate the input script (and time execution if requested)
    let start_time = if performance {
        Some(std::time::Instant::now())
    } else {
        None
    };

    let eval_result = context.eval(&code, false);

    if let Some(start) = start_time {
        let elapsed = start.elapsed();
        println!("Execution time: {:.3} ms", elapsed.as_secs_f64() * 1000.0);
    }

    match eval_result {
        Ok(value) => {
            if !value.is_undefined() {
                match value.js_to_string() {
                    Ok(s) => println!("{}", s),
                    Err(_) => println!("{:?}", value),
                }
            }
        }
        Err(err) => {
            eprintln!("Execution Error: {}", err);
            std::process::exit(1);
        }
    }
}
