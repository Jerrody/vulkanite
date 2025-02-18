use std::{collections::HashSet, ffi::CString, iter};

use anyhow::{anyhow, Result};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{Ident, LitCStr};

use crate::{
    structs::{Command, ReturnType},
    xml,
};

use super::Generator;

struct DispEntry {
    /// Main function name (e.g cmd_set_line_stipple)
    name: Ident,
    /// Main name and all possible aliases (e.g cmd_set_line_stipple, cmd_set_line_stipple_khr, ...)
    names: Vec<Ident>,
    /// All params used by a function (e.g for cmd_set_line_stipple: [ Option<BorrowedHandle<'_, CommandBuffer>>, u32, u16 ])
    params: Vec<TokenStream>,
    /// Return type of the entry (can be nothing, -> Status or -> type)
    ret_ty: TokenStream,
}

pub fn generate<'a>(gen: &Generator<'a>) -> Result<String> {
    let mut listed_commands = HashSet::new();
    let mut proc_addr_loader = Vec::new();
    let mut instance_loader = Vec::new();
    let mut device_loader = Vec::new();

    let dispatcher_features = gen.filtered_features().flat_map(|feat| &feat.require);
    let dispatcher_extensions = gen
        .filtered_extensions()
        .flat_map(|ext: &xml::Extension| &ext.require);

    let dispatcher_entries = dispatcher_features
        .chain(dispatcher_extensions)
        .flat_map(|req| &req.content)
        .filter_map(|cnt| match cnt {
            xml::RequireContent::Command(cmd) => Some(cmd),
            _ => None,
        })
        .filter(|cmd| listed_commands.insert(&cmd.name))
        .filter_map(|cmd| gen.commands.get(cmd.name.as_str()))
        .map(|cmd| {
            generate_dispatch_entry(
                gen,
                cmd,
                &mut proc_addr_loader,
                &mut instance_loader,
                &mut device_loader,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let ent_name: Vec<_> = dispatcher_entries.iter().map(|ent| &ent.name).collect();
    let ent_names: Vec<_> = dispatcher_entries
        .iter()
        .map(|ent| ent.names.as_slice())
        .collect();
    let ent_params: Vec<_> = dispatcher_entries
        .iter()
        .map(|ent| ent.params.clone())
        .collect();
    let ent_ret_ty: Vec<_> = dispatcher_entries.iter().map(|ent| &ent.ret_ty).collect();

    let ent_defs = ent_params
        .iter()
        .zip(&ent_ret_ty)
        .map(|(params, ret_ty)| quote! (unsafe extern "system" fn (#(#params),*) #ret_ty));

    let result = quote! {
        // Will be replaced by the appropriate comment below
        improper_ctypes_comment
        #![allow(improper_ctypes)]

        use crate::*;
        use crate::vk::*;
        use crate::vk::raw::*;

        use std::mem;
        use std::cell::Cell;
        use std::ffi::{c_char, c_int, c_void};

        #[derive(Clone)]
        pub struct CommandsDispatcher {
            #(
                #(pub #ent_names: Cell<#ent_defs>,)*
            )*
        }
        /// SAFETY: This trait is safe to implement assuming setting an aligned pointer-size value is coherent (another thread can see the previous value
        /// or the new value but not for example 4 bytes of the previous value then 4 bytes of the new value if the pointer is 8 bytes).
        /// This really weak memory property is true for most CPU memory models (even ARM)
        /// Otherwise you would have to mark functions modifying the dispatcher (instance, device creation) as not thread safe, but we don't do it 
        unsafe impl Sync for CommandsDispatcher{}

        impl CommandsDispatcher {
            pub unsafe fn load_proc_addr(&self, get_instance_proc_addr: unsafe extern "system" fn(Option<BorrowedHandle<'_, Instance>>, *const c_char) -> FuncPtr) {
                self.get_instance_proc_addr.set(get_instance_proc_addr);

                #(#proc_addr_loader)*
            }

            pub unsafe fn load_instance(&self, instance: &Instance) {
                let get_instance_proc_addr = self.get_instance_proc_addr.get();

                #(#instance_loader)*
            }

            pub unsafe fn load_device(&self, device: &Device) {
                let get_device_proc_addr = self.get_device_proc_addr.get();

                #(#device_loader)*;
            }
        }

        impl CommandsDispatcher {
            /// Create a new dispatcher
            /// On creation, all functions from the dispatcher will point to a placeholder
            /// Which will panic when called
            /// The dispatcher has to be loaded (see [Self::load_proc_addr] and [Self::load_instance]) before being used
            pub const fn new() -> Self {
                Self::new_inner()
            }

            #[cfg(all(
                not(all(target_arch="x86", target_family="windows")),
                all(
                    any(target_arch="x86", target_arch="x86_64", target_arch="arm", target_arch="aarch64"),
                    any(target_family="windows", target_family="unix"),
                ),
            ))]
            const fn new_inner() -> Self {
                /// Note that because this function is not unwind
                /// Calling this function will abort the current process
                extern "system" fn on_unloaded_command() {
                    panic!("Trying to call an unloaded Vulkan command");
                }
                let unload_cmd = on_unloaded_command as *const ();

                // Will be replaced by the appropriate comment below
                new_inner_safety_arg
                unsafe {
                    Self {
                        #(
                            #(#ent_names: Cell::new(mem::transmute(unload_cmd)),)*
                        )*
                    }
                }
            }

            #[cfg(not(all(
                not(all(target_arch="x86", target_family="windows")),
                all(
                    any(target_arch="x86", target_arch="x86_64", target_arch="arm", target_arch="aarch64"),
                    any(target_family="windows", target_family="unix"),
                ),
            )))]
            const fn new_inner() -> Self {
                #(
                    extern "system" fn #ent_name(#(_: #ent_params),*) #ent_ret_ty {
                        panic!("Trying to call an unloaded Vulkan command");
                    }
                )*
                Self {
                    #(
                        #(#ent_names: Cell::new(#ent_name),)*
                    )*
                }
            }
        }
    }
    .to_string();

    let result = result.replace("improper_ctypes_comment", "
        // Some vulkan functions take as input arrays of basic types [f32; 4]...
        // Rust considers this as not FFI-safe
        // Hopefully, when actually compiled, it matches the layout expected by the vulkan function (looks like it so far))
    ");
    let result = result.replace("new_inner_safety_arg", "
        // SAFETY: On platforms specified by the config directive above
        // The stack is caller restored. This means that calling a \"system\" function with 0
        // argument while its signature is defined to have multiple is actually defined behavior
        // Moreover, the return value of a Vulkan function always go in a register for these platforms
        // As such, calling on_unloaded_command (no param and and not return value) should be safe on these platforms
        // If you believe part of this reasoning is wrong, feel free to open an issue
    ");

    Generator::format_result(result)
}

fn generate_dispatch_entry<'a>(
    gen: &Generator<'a>,
    cmd: &Command<'a>,
    proc_addr_loader: &mut Vec<TokenStream>,
    instance_loader: &mut Vec<TokenStream>,
    device_loader: &mut Vec<TokenStream>,
) -> Result<DispEntry> {
    let ret_ty = match cmd.return_ty {
        ReturnType::Void => quote!(),
        ReturnType::Result { .. } => quote! (-> Status),
        ReturnType::BaseType(name) => {
            let name = gen
                .mapping
                .borrow()
                .get(name)
                .map(|entry| format_ident!("{}", entry.name))
                .ok_or_else(|| anyhow!("Failed to find type {name}"))?;
            quote! (-> #name)
        }
    };

    let params = cmd
        .params
        .iter()
        .map(|param| gen.generate_type_inner(&param.advanced_ty.get().unwrap(), false))
        .collect::<Result<Vec<_>>>()?;

    let aliases = cmd.aliases.borrow();

    let only_proc = cmd.handle.get().is_none();
    let only_instance = cmd
        .handle
        .get()
        .is_some_and(|name| name == "VkInstance" || name == "VkPhysicalDevice");

    // If someone has a nicer way to write this function, feel free to modify it
    let mut push_loaders = |instr: TokenStream, is_get_addr: bool| {
        if only_proc {
            if is_get_addr {
                proc_addr_loader.push(quote! (get_instance_proc_addr(None, #instr.as_ptr())));
            } else {
                proc_addr_loader.push(instr);
            }
        } else {
            if is_get_addr {
                instance_loader.push(
                    quote! (get_instance_proc_addr(Some(instance.borrow()), #instr.as_ptr())),
                );
            } else {
                instance_loader.push(instr.clone());
            }

            if !only_instance {
                if is_get_addr {
                    device_loader.push(
                        quote! (get_device_proc_addr(Some(device.borrow()), #instr.as_ptr())),
                    );
                } else {
                    device_loader.push(instr);
                }
            }
        }
    };

    let main_fn_name = format_ident!("{}", cmd.name);
    push_loaders(
        quote! (let mut vk_func_ptr = self.#main_fn_name.get();),
        false,
    );

    let mut fn_names = vec![];

    // aliases should appear in the order khr, ext, vendors
    // so look at them in reverse order (if both ext and khr is available but not the main (no prefix) function,
    // use the khr version)
    for (vk_name, name) in aliases
        .iter()
        .rev()
        .map(|(vk_name, alias)| (*vk_name, format_ident!("{alias}")))
        .chain(iter::once((cmd.vk_name, main_fn_name.clone())))
    {
        let name_cstr = LitCStr::new(&CString::new(vk_name).unwrap(), Span::call_site());
        push_loaders(quote! (let loaded_ptr = ), false);
        push_loaders(name_cstr.into_token_stream(), true);
        push_loaders(
            quote! {
                ;
                if !loaded_ptr.is_null() {
                    vk_func_ptr = mem::transmute(loaded_ptr);
                }
                self.#name.set(vk_func_ptr);
            },
            false,
        );

        fn_names.push(name);
    }

    Ok(DispEntry {
        name: main_fn_name,
        names: fn_names,
        params,
        ret_ty,
    })
}
