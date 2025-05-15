use wasmer::{FunctionEnvMut, MemoryView, StoreRef, WasmPtr};

use crate::executor::Env;

use super::FunctionErrors;

/// When a host function is executing we need to be able to access the guest's memory
/// for read and write operations. This safely gets those from the environment and
/// handles all failure cases.
pub fn get_memory<'a>(
    env: &FunctionEnvMut<Env>,
    store: &'a StoreRef,
) -> Result<MemoryView<'a>, FunctionErrors> {
    // Fetch the store and memory which make up the needed components
    // of the execution environment.
    let memory = match &env.data().memory {
        Some(m) => m,
        None => {
            error!("Memory was not initialized for a function call!?");
            return Err(FunctionErrors::InternalApiError);
        }
    };

    // Turn the memory into a view so we can read and write to the
    // underlying memory.
    let memory_view = memory.view(store);

    Ok(memory_view)
}

/// Safely get a string from the guest's memory. This function will take a pointer provided by the
/// guest, then use a built in function to read the string.
pub fn safely_get_string(
    memory_view: &MemoryView,
    data_buffer: WasmPtr<u8>,
    buffer_size: u32,
) -> Result<String, FunctionErrors> {
    match data_buffer.read_utf8_string(&memory_view, buffer_size as u32) {
        Ok(s) => Ok(s),
        Err(_) => {
            error!("Failed to read the log message from the guest's memory");
            Err(FunctionErrors::ParametersNotUtf8)
        }
    }
}

/// Safely get a Vec<u8> from the guest's memory. This function will take a pointer provided by the
/// guest, then do a bounds checked read.
pub fn safely_get_memory(
    memory_view: &MemoryView,
    data_buffer: WasmPtr<u8>,
    buffer_size: u32,
) -> Result<Vec<u8>, FunctionErrors> {
    let mut buffer = vec![0; buffer_size as usize];
    memory_view
        .read(data_buffer.offset().into(), &mut buffer)
        .map_err(|_| FunctionErrors::CouldNotGetAdequateMemory)?;

    Ok(buffer)
}

/// Safely write data back to the guest's memory. This function will take a pointer provided
/// to it by the guest, do some bounds checking, and then write the data back into the guest's
/// memory. It will return the number of bytes written or an error if the buffer is too small.
pub fn safely_write_data_back(
    memory_view: &MemoryView,
    data: &[u8],
    data_buffer: WasmPtr<u8>,
    buffer_size: u32,
) -> Result<i32, FunctionErrors> {
    if buffer_size == 0 {
        return Ok(data.len() as i32);
    }

    if data.len() > buffer_size as usize {
        return Err(FunctionErrors::ReturnBufferTooSmall);
    }

    let values = data_buffer
        .slice(&memory_view, data.len() as u32)
        .map_err(|_| FunctionErrors::CouldNotGetAdequateMemory)?;

    for i in 0..data.len() {
        if let Err(_) = values.index(i as u64).write(data[i]) {
            return Err(FunctionErrors::FailedToWriteGuestMemory);
        }
    }

    Ok(data.len() as i32)
}
