use std::cell::RefCell;

use rand_chacha::ChaCha8Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};

const RESEED_INTERVAL: u32 = 32;

#[derive(Debug)]
pub struct Trng {
    csprng: RefCell<rand_chacha::ChaCha8Rng>,
    reseed_ctr: RefCell<u32>,
}
impl Trng {
    pub fn new(_xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        Ok(Trng {
            csprng: RefCell::new(ChaCha8Rng::seed_from_u64(
                (xous::create_server_id().unwrap().to_u32().0 as u64)
                    | ((xous::create_server_id().unwrap().to_u32().0 as u64) << 32),
            )),
            reseed_ctr: RefCell::new(0),
        })
    }

    fn reseed(&self) {
        *self.reseed_ctr.borrow_mut() = *self.reseed_ctr.borrow() + 1;
        if *self.reseed_ctr.borrow() > RESEED_INTERVAL {
            *self.reseed_ctr.borrow_mut() = 0;
            // incorporate randomness from the TRNG
            let half = self.csprng.borrow_mut().next_u32();
            self.csprng.replace(rand_chacha::ChaCha8Rng::seed_from_u64(
                (half as u64) << 32 | (xous::create_server_id().unwrap().to_u32().0 as u64),
            ));
        }
    }

    pub fn get_u32(&self) -> Result<u32, xous::Error> {
        self.reseed();
        Ok(self.csprng.borrow_mut().next_u32())
    }

    pub fn get_u64(&self) -> Result<u64, xous::Error> {
        self.reseed();
        Ok(self.csprng.borrow_mut().next_u64())
    }

    pub fn fill_buf(&self, data: &mut [u32]) -> Result<(), xous::Error> {
        for d in data.iter_mut() {
            *d = self.get_u32()?;
        }
        Ok(())
    }

    /// This is copied out of the 0.5 API for rand_core
    pub fn fill_bytes_via_next(&mut self, dest: &mut [u8]) {
        use core::mem::transmute;
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;
            let chunk: [u8; 8] = unsafe { transmute(self.next_u64().to_le()) };
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = unsafe { transmute(self.next_u64().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = unsafe { transmute(self.next_u32().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        }
    }
}

impl RngCore for Trng {
    fn next_u32(&mut self) -> u32 { self.get_u32().expect("couldn't get random u32 from server") }

    fn next_u64(&mut self) -> u64 { self.get_u64().expect("couldn't get random u64 from server") }

    fn fill_bytes(&mut self, dest: &mut [u8]) { self.fill_bytes_via_next(dest); }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Ok(self.fill_bytes(dest))
    }
}

impl CryptoRng for Trng {}
