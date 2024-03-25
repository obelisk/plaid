use crate::PlaidFunctionError;

#[derive(Debug)]
pub enum OtpStatus {
    Ok,
    BadOtp,
    ReplayedOtp,
    Unknown(String),
}

pub fn verify_otp(otp: &str) -> bool {
    extern {
        // Verify an OTP
        new_host_function_with_error_buffer!(yubikey, verify_otp);
    }

    let otp_bytes = otp.as_bytes().to_vec();
    let mut return_buffer = vec![0; 1024];

    let res = unsafe {
        yubikey_verify_otp(
            otp_bytes.as_ptr(),
            otp_bytes.len(),
            return_buffer.as_mut_ptr(),
            1024
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return false
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let return_string = String::from_utf8(return_buffer).unwrap();

    match return_string.as_str() {
        "OK" => true,
        _ => false,
    }
}

pub fn verify_otp_detailed(otp: &str) -> Result<OtpStatus, PlaidFunctionError> {
    extern {
        // Verify an OTP
        new_host_function_with_error_buffer!(yubikey, verify_otp);
    }

    let otp_bytes = otp.as_bytes().to_vec();
    let mut return_buffer = vec![0; 1024];

    let res = unsafe {
        yubikey_verify_otp(
            otp_bytes.as_ptr(),
            otp_bytes.len(),
            return_buffer.as_mut_ptr(),
            1024
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into())
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let return_string = String::from_utf8(return_buffer).unwrap();

    match return_string.as_str() {
        "OK" => Ok(OtpStatus::Ok),
        "BAD_OTP" => Ok(OtpStatus::BadOtp),
        "REPLAYED_OTP" => Ok(OtpStatus::ReplayedOtp),
        x => Ok(OtpStatus::Unknown(x.to_string())),
    }
}
