# LokanOS – Lokan Home Hub Runtime

## TL;DR
- Headless, modular Rust runtime for the Lokan Home Hub.
- Provides services, device drivers, automation rules, and network connectors.
- Delivered in phased milestones documented under `docs/`.

LokanOS is a universal, headless operating framework for the Lokan Home Hub.
It provides a modular runtime for orchestrating device drivers, automation
rules, and network protocol connectors without a traditional UI.  The runtime
is implemented as a Rust workspace composed of focused crates that can be
embedded into firmware images or extended with new services.

## Workspace layout

| Crate | Purpose |
| ----- | ------- |
| `lokan-core` | Runtime primitives (service lifecycle, configuration) |
| `lokan-event` | Broadcast event bus shared between services |
| `lokan-device` | Device abstraction layer and registry |
| `lokan-automation` | Rule engine for event-driven automations |
| `lokan-network` | Extensible protocol connector interfaces |
| `hub-daemon` | Example headless daemon wiring all building blocks |

## Getting started

The `hub-daemon` crate demonstrates how to compose the framework into a
self-contained runtime:

```bash
cargo run -p hub-daemon
```

The daemon boots the service manager, registers a mock temperature sensor, and
spawns an automation service that reacts to emitted telemetry.  Press `Ctrl+C`
to stop the runtime gracefully.

## Extending the framework

- Add new services by implementing the `lokan_core::Service` trait and
  registering them with `ServiceManager`.
- Share cross-cutting resources (for example, protocol clients) by attaching
  them to the `ServiceContext` via extensions.
- Implement additional device drivers by conforming to the
  `lokan_device::DeviceDriver` trait and registering devices with the
  `DeviceRegistry`.
- Create domain-specific automations by adding new rules to the
  `lokan_automation::RuleEngine`.

The modular workspace enables tailoring LokanOS to a variety of smart-home hub
form factors while keeping the core runtime headless and lightweight.
