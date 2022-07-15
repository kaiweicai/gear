initSidebarItems({"attr":[["async_init","Mark async function to be the program initialization method. Can be used together with [`async_main`]. Functions `init`, `handle_reply` cannot be specified if this macro is used. If you need to specify `init`, `handle_reply` explicitly don’t use this macro."],["async_main","This is the procedural macro for your convenience. It marks the main async function to be the program entry point. Functions `handle`, `handle_reply` cannot be specified if this macro is used. If you need to specify `handle`, `handle_reply` explicitly don’t use this macro."]],"fn":[["message_loop","Gear allows users and programs to interact with other users and programs via messages. This function enables an asynchronous message handling main loop."],["record_reply",""]],"macro":[["bail",""],["debug",""],["export",""],["metadata",""]],"mod":[["errors","Gear common errors module. Enumerates errors that can occur in smart-contracts `ContractError`. Errors related to conversion, decoding, message exit code, other internal errors."],["exec","Sys calls related to the program execution flow."],["lock","Async lockers primitives."],["macros","Gear macros."],["msg","Messaging module."],["prelude","The `gstd` default prelude. Re-imports default `std` modules and traits. `std` can be safely replaced to `gstd` in the Rust programs."],["prog","Program creation module."]],"struct":[["ActorId","Program (actor) identifier."],["CodeHash",""],["MessageId","Message identifier."]]});