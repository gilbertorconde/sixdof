//! Prints what the device reports, by whichever route reaches it, for
//! checking a device by hand.
//!
//! ```text
//! cargo run --example watch
//! ```

/// Reads the daemon's settings, when a daemon is what answered. Nothing here
/// writes: these belong to every application on the machine.
#[cfg(unix)]
fn print_settings(source: &mut sixdof::Source) {
    let Some(mut config) = source.config() else {
        return;
    };
    println!("daemon settings:");
    println!("  sensitivity      {:?}", config.sensitivity());
    println!("  per axis         {:?}", config.axis_sensitivity());
    println!("  inverted         {:?}", config.inverted());
    println!("  dead zone axis 0 {:?}", config.dead_zone(0));
    println!("  axis 0 maps to   {:?}", config.axis_map(0));
    println!("  button 0 maps to {:?}", config.button_map(0));
    println!("  button 0 action  {:?}", config.button_action(0));
    println!("  swap y and z     {:?}", config.swap_yz());
    println!("  led              {:?}", config.led());
    println!("  grab device      {:?}", config.grab_device());
    println!("  repeat           {:?}", config.repeat_interval());
    println!("  serial device    {:?}", config.serial_device());
    println!("  socket           {:?}", config.socket_path());
}

#[cfg(not(unix))]
fn print_settings(_: &mut sixdof::Source) {}

fn main() {
    let mut source = match sixdof::Source::connect() {
        Ok(source) => source,
        Err(err) => {
            eprintln!("no device reachable: {err}");
            std::process::exit(1);
        }
    };

    println!("connected by {}", source.backend());
    match source.device() {
        Some(device) => println!(
            "device {:?} ({} axes, {} buttons, usb {:?}, type {:#x}) at {:?}",
            device.name,
            device.axes,
            device.buttons,
            device.usb_id,
            device.device_type,
            device.path
        ),
        None => println!("no device details on this route"),
    }

    print_settings(&mut source);

    source.set_name("sixdof watch").ok();
    source.set_event_mask(sixdof::EventMask::DEFAULT).ok();

    loop {
        match source.read_blocking() {
            Ok(sixdof::Event::Device { added, .. }) => {
                println!("device {}", if added { "added" } else { "removed" });
                match source.refresh_device() {
                    Ok(Some(device)) => println!("  now {:?}", device.name),
                    Ok(None) => println!("  now nothing"),
                    Err(err) => println!("  query failed: {err}"),
                }
            }
            Ok(event) => println!("{event:?}"),
            Err(err) => {
                eprintln!("{err}");
                return;
            }
        }
    }
}
