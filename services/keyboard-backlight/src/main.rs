#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use crossbeam::channel::{at, select, unbounded, Receiver, Sender};
use num_traits::*;
use std::sync::Arc;
use std::sync::Mutex;

enum ThreadOps {
    Renew,
    Stop,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let kbb_sid = xns
        .register_name(api::KBB_SERVER_NAME, Some(2))
        .expect("can't register server");

    // connect to com
    let com = com::Com::new(&xns).expect("cannot connect to com");

    let keyboard = keyboard::Keyboard::new(&xns).expect("cannot connect to keyboard server");
    keyboard.register_observer(
        "_Keyboard backlight_",
        KbbOps::Keypress.to_u32().unwrap() as usize,
    );
    
    let enabled = Arc::new(Mutex::new(false));
    let (tx, rx): (Sender<ThreadOps>, Receiver<ThreadOps>) = unbounded();

    let rx = Box::new(rx);

    let thread_already_running = Arc::new(Mutex::new(false));
    let thread_conn = xous::connect(kbb_sid).unwrap();

    loop {
        let msg = xous::receive_message(kbb_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(KbbOps::Keypress) => {
                if !*enabled.lock().unwrap() {
                    log::trace!("ignoring keypress, automatic backlight is disabled");
                    continue
                }
                let mut run_lock = thread_already_running.lock().unwrap();
                match *run_lock {
                    true => {
                        log::trace!("renewing backlight timer");
                        tx.send(ThreadOps::Renew).unwrap();
                        continue
                    },
                    false => {
                        *run_lock = true;
                        com.set_backlight(255, 128).expect("cannot set backlight on");
                        std::thread::spawn({
                            let rx = rx.clone();
                            move || turn_lights_on(rx, thread_conn)
                        });
                    },
                }
            }
            Some(KbbOps::TurnLightsOn) => {
                log::trace!("turning lights on");
                com.set_backlight(255, 128).expect("cannot set backlight on");
            },
            Some(KbbOps::TurnLightsOff) => {
                log::trace!("turning lights off");
                let mut run_lock = thread_already_running.lock().unwrap();
                *run_lock = false;
                com.set_backlight(0, 0).expect("cannot set backlight off");
            },
            Some(KbbOps::EnableAutomaticBacklight) => {
                *enabled.lock().unwrap() = true;
            }
            Some(KbbOps::DisableAutomaticBacklight) => {
                *enabled.lock().unwrap() = false;
                tx.send(ThreadOps::Stop).unwrap();
            }
            Some(KbbOps::Status) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let status = *enabled.lock().unwrap();
                xous::return_scalar(msg.sender, status.into()).unwrap();
            }),
            _ => {}
        }
    }
}

fn turn_lights_on(rx: Box<Receiver<ThreadOps>>, cid: xous::CID) {
    let standard_duration = std::time::Duration::from_secs(10);

    let mut timeout = std::time::Instant::now() + standard_duration;

    let mut total_waited = 0;

    loop {
        select! {
            recv(rx) -> op => {
                match op.unwrap() {
                    ThreadOps::Renew => {
                        timeout = std::time::Instant::now() + standard_duration;
                        total_waited += 1;
                    },
                    ThreadOps::Stop => {
                        log::trace!("received Stop op, killing background backlight thread");
                        return;
                    },
                }
            },
            recv(at(timeout)) -> _ => {
                log::trace!("timeout finished, total re-waited {}, returning!", total_waited);
                xous::send_message(cid, xous::Message::new_scalar(KbbOps::TurnLightsOff.to_usize().unwrap(), 0,0,0,0)).unwrap();
                break;
            }
        };
    }
}
