//! WLED Matter: WLED strips as Matter lights for Yandex Station («Дом с Алисой»).
//!
//! Every configured WLED strip becomes a Matter extended color light (on/off, brightness,
//! colour, colour temperature). Each strip is a separate Matter node with its own port, storage
//! and pairing code, so it can be paired in the Yandex app on its own. Built on rs-matter's
//! color_light example; uses the SDK test attestation (VID 0xFFF1). Strips are configured from
//! the web UI (see `http.rs`).
//!
//! Env: WLED_HOST (strip of a fresh install, 192.168.3.43), MATTER_IFACE (wlan0),
//!      MATTER_STORAGE (/data/yam/var/wled_matter), HTTP_PORT (8097; 8098 is wled_anim,
//!      8099 is Luna), RUST_LOG (info).

// The strip's handler chain makes deeply nested future types
#![recursion_limit = "256"]

use core::pin::pin;

use std::net::{Ipv6Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::thread;

use embassy_futures::select::select3;
use embassy_time::{Duration, Timer};

use log::{error, info, warn};

use rand::Rng;
use rs_matter::crypto::{default_crypto, Crypto};
use rs_matter::dm::clusters::app::color_control::{
    self, ColorCapabilitiesBitmap, ColorControlHooks, RgbGamma, SetDeviceColor,
};
use rs_matter::dm::clusters::app::level_control::{
    self, LevelControlHooks, OutOfBandMessage as LevelMessage,
};
use rs_matter::dm::clusters::app::on_off::{self, OnOffHooks, OutOfBandMessage as OnOffMessage};
use rs_matter::dm::clusters::basic_info::BasicInfoConfig;
use rs_matter::dm::clusters::decl::color_control as color_control_cluster;
use rs_matter::dm::clusters::decl::level_control::{
    AttributeId, CommandId, OptionsBitmap, FULL_CLUSTER as LEVEL_CONTROL_FULL_CLUSTER,
};
use rs_matter::dm::clusters::decl::on_off as on_off_cluster;
use rs_matter::dm::clusters::decl::scenes_management::FULL_CLUSTER as SCENES_FULL_CLUSTER;
use rs_matter::dm::clusters::desc::{self, ClusterHandler as _};
use rs_matter::dm::clusters::groups::{self, ClusterHandler as _};
use rs_matter::dm::clusters::identify::{self, IdentifyHandler};
use rs_matter::dm::clusters::scenes::{SceneClusters, ScenesHandler, ScenesState};
use rs_matter::dm::devices::test::{DAC_PRIVKEY, TEST_DEV_ATT, TEST_DEV_DET};
use rs_matter::dm::devices::DEV_TYPE_EXTENDED_COLOR_LIGHT;
use rs_matter::dm::endpoints;
use rs_matter::dm::networks::SysNetifs;
use rs_matter::dm::{Async, Cluster, DataModel, Dataver, Endpoint, Node};
use rs_matter::error::Error;
use rs_matter::im::{InteractionModel, InteractionModelState};
use rs_matter::pairing::qr::{no_optional_data, CommFlowType, NoOptionalData, QrPayload};
use rs_matter::pairing::DiscoveryCapabilities;
use rs_matter::respond::DefaultResponder;
use rs_matter::sc::pase::{CommWindowState, MAX_COMM_WINDOW_TIMEOUT_SECS};
use rs_matter::sc::pase::{Spake2pVerifierPassword, Spake2pVerifierPasswordRef};
use rs_matter::transport::exchange::MatterBuffers;
use rs_matter::utils::select::Coalesce;
use rs_matter::{clusters, devices, root_endpoint, with, BasicCommData, Matter, MATTER_PORT};

mod config;
mod http;
mod mdns;
mod wled;
mod wled_anim;

use config::{Config, Strip};
use wled::Wled;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const SCENES_CAPACITY: usize = 16;

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

// ---- Device status shared with the web API ----

#[derive(Clone, Debug, serde::Serialize)]
pub struct DevStatus {
    pub id: String,
    pub name: String,
    pub commissioned: bool,
    pub code: String,
    pub qr: String,
}

pub static STATUS: Mutex<Vec<DevStatus>> = Mutex::new(Vec::new());
pub static RESTARTING: AtomicBool = AtomicBool::new(true);
/// Bumped to make every running node shut down (config reload / unpair)
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn set_commissioned(id: &str, commissioned: bool) {
    if let Some(s) = STATUS.lock().unwrap().iter_mut().find(|s| s.id == id) {
        s.commissioned = commissioned;
    }
}

pub enum Request {
    Reload,
    Unpair(String),
}

pub struct Ctx {
    pub base: PathBuf,
    pub iface: String,
    pub default_host: String,
}

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );
    info!("WLED Matter {VERSION}");

    let ctx: &'static Ctx = Box::leak(Box::new(Ctx {
        base: PathBuf::from(env("MATTER_STORAGE", "/data/yam/var/wled_matter")),
        iface: env("MATTER_IFACE", "wlan0"),
        default_host: env("WLED_HOST", "192.168.3.43"),
    }));
    if let Err(e) = std::fs::create_dir_all(&ctx.base) {
        error!("{}: {e}", ctx.base.display());
        std::process::exit(1);
    }

    let (tx, rx) = channel::<Request>();
    let http_port: u16 = env("HTTP_PORT", "8097").parse().unwrap_or(8097);
    http::start(ctx, http_port, tx);
    wled_anim::start_watch(move || Config::load(&ctx.base, &ctx.default_host));

    supervise(ctx, rx);
}

/// Runs all configured strips; on a request stops them, applies it and starts them again.
fn supervise(ctx: &'static Ctx, rx: Receiver<Request>) {
    loop {
        let cfg = Config::load(&ctx.base, &ctx.default_host);
        remove_stale_storage(ctx, &cfg);
        let gen = GENERATION.load(Ordering::SeqCst);
        let handles = start_nodes(ctx, &cfg, gen);
        RESTARTING.store(false, Ordering::SeqCst);

        let Ok(req) = rx.recv() else { return };
        RESTARTING.store(true, Ordering::SeqCst);
        GENERATION.fetch_add(1, Ordering::SeqCst);
        for h in handles {
            let _ = h.join();
        }
        if let Request::Unpair(id) = req {
            info!("Unpairing {id}");
            let _ = std::fs::remove_dir_all(ctx.base.join(&id));
        }
    }
}

/// Strips deleted from the config (or devices of older versions) keep no pairing behind
fn remove_stale_storage(ctx: &Ctx, cfg: &Config) {
    let Ok(entries) = std::fs::read_dir(&ctx.base) else { return };
    for e in entries.flatten().filter(|e| e.path().is_dir()) {
        let id = e.file_name().to_string_lossy().to_string();
        if cfg.strip(&id).is_none() {
            info!("Removing storage of deleted device {id}");
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

fn start_nodes(ctx: &'static Ctx, cfg: &Config, gen: u64) -> Vec<thread::JoinHandle<()>> {
    STATUS.lock().unwrap().clear();
    cfg.strips
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, strip)| {
            let port = MATTER_PORT + index as u16;
            thread::Builder::new()
                .name(format!("node{index}"))
                .stack_size(512 * 1024)
                .spawn(move || {
                    let id = strip.id.clone();
                    if let Err(e) = run_node(ctx, strip, index as u8, port, gen) {
                        error!("Strip {id} stopped: {e:?}");
                    }
                })
                .expect("spawn node thread")
        })
        .collect()
}

fn run_node(ctx: &Ctx, strip: Strip, index: u8, port: u16, gen: u64) -> Result<(), Error> {
    let id = strip.id.clone();
    let dir = ctx.base.join(&id);
    std::fs::create_dir_all(&dir)?;
    let (passcode, discriminator) = config::comm_data(&dir);
    let comm = BasicCommData {
        password: Spake2pVerifierPassword::new_from_ref(Spake2pVerifierPasswordRef::new(
            &passcode.to_le_bytes(),
        )),
        discriminator,
    };
    // The original single-strip bridge was paired as "wled-matter-1"; keep that identity
    let unique_id = if id == "s1" { "wled-matter-1".to_string() } else { format!("wled-{id}") };
    let det = BasicInfoConfig {
        vendor_name: "WLED",
        product_name: "WLED Strip",
        product_label: &strip.name,
        device_name: &strip.name,
        serial_no: &unique_id,
        unique_id: &unique_id,
        sw_ver_str: VERSION,
        ..TEST_DEV_DET
    };

    let mut qr_buf = [0u8; 256];
    let qr = QrPayload::new_from_basic_info(
        DiscoveryCapabilities::IP,
        CommFlowType::Standard,
        comm.clone(),
        &det,
        no_optional_data as NoOptionalData,
    )
    .as_str(&mut qr_buf)
    .map(|(text, _)| text.to_string())
    .unwrap_or_default();

    let matter = Matter::new(&det, comm.clone(), &TEST_DEV_ATT, port);
    let store = rs_matter::persist::DirKvBlobStore::new(dir);
    // Every live subscription pins one buffer and Yandex opens ~9 of them; with the default
    // pool of 10 every command got a `Busy` reply. ~1.6 KB each.
    let buffers: MatterBuffers<32> = MatterBuffers::new();
    let state: InteractionModelState = InteractionModelState::new();
    let kv = matter.kv(store);
    matter.startup(&kv)?;

    STATUS.lock().unwrap().push(DevStatus {
        id: id.clone(),
        name: strip.name.clone(),
        commissioned: matter.has_fabrics(),
        code: comm.compute_pretty_pairing_code().to_string(),
        qr,
    });
    info!("Strip {id} «{}» ({}) on port {port}, paired: {}", strip.name, strip.host, matter.has_fabrics());

    let crypto = default_crypto(rand::rng(), DAC_PRIVKEY);
    let mut rand = crypto.rand()?;
    // Hooks get called with the restored state during startup; that must not touch the strip
    let ready = AtomicBool::new(false);
    let wled = Wled::get(&strip.host);

    let scenes_state = ScenesState::<SCENES_CAPACITY>::new();
    let on_off_handler = on_off::OnOffHandler::new(
        Dataver::new_rand(&mut rand),
        1,
        rs_matter::persist::VENDOR_KEYS_START + 0x10,
        OnOffLogic { wled, ready: &ready },
    )
    .with_scene_invalidator(&scenes_state);
    let level_control_handler = level_control::LevelControlHandler::new(
        Dataver::new_rand(&mut rand),
        1,
        rs_matter::persist::VENDOR_KEYS_START + 0x11,
        LevelLogic { wled, ready: &ready },
    )
    .with_scene_invalidator(&scenes_state);
    let color_control_handler = color_control::ColorControlHandler::new(
        Dataver::new_rand(&mut rand),
        1,
        rs_matter::persist::VENDOR_KEYS_START + 0x12,
        ColorLogic { wled, ready: &ready },
    )
    .with_scene_invalidator(&scenes_state);
    on_off_handler.init(Some(&level_control_handler));
    level_control_handler.init(Some(&on_off_handler));
    color_control_handler.init(Some(&on_off_handler));
    let scenes_handler = ScenesHandler::new(
        Dataver::new_rand(&mut rand),
        &scenes_state,
        (
            &on_off_handler,
            (&level_control_handler, (&color_control_handler, ())),
        ),
    );
    let im = InteractionModel::new(
        &matter,
        &crypto,
        &buffers,
        strip_data_model(
            rand,
            &on_off_handler,
            &level_control_handler,
            &color_control_handler,
            scenes_handler,
        ),
        &kv,
        &state,
    );
    futures_lite::future::block_on(im.startup())?;
    ready.store(true, Ordering::SeqCst);

    let responder = DefaultResponder::new(&im);
    let mut respond = pin!(responder.run::<4, 4>());
    let mut im_job = pin!(im.run());
    let socket = async_io::Async::<UdpSocket>::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)))?;
    let mut mdns = pin!(mdns::run_mdns(&matter, &crypto, &ctx.iface, index));
    let mut transport = pin!(matter.run(&crypto, &socket, &socket, &socket));
    let mut watch = pin!(watch(&matter, &crypto, gen, &id));
    let all = select3(
        &mut transport,
        &mut mdns,
        select3(&mut respond, &mut im_job, &mut watch).coalesce(),
    );
    futures_lite::future::block_on(all.coalesce())
}

/// Publishes the pairing state, keeps an unpaired strip commissionable (the basic window times
/// out after 15 min, and removing the device in Yandex drops its fabric) and ends the node when
/// the supervisor bumps the generation.
async fn watch<C: Crypto>(matter: &Matter<'_>, crypto: C, gen: u64, id: &str) -> Result<(), Error> {
    let mut last = None;
    loop {
        if GENERATION.load(Ordering::SeqCst) != gen {
            info!("Strip {id} stopping");
            return Ok(());
        }
        let paired = matter.has_fabrics();
        if last != Some(paired) {
            set_commissioned(id, paired);
            last = Some(paired);
        }
        if !paired && matches!(matter.comm_window_state(), CommWindowState::Closed) {
            match matter.open_basic_comm_window(MAX_COMM_WINDOW_TIMEOUT_SECS, &crypto, &()) {
                Ok(()) => info!("Strip {id}: commissioning window open"),
                Err(e) => warn!("Strip {id}: cannot open commissioning window: {e:?}"),
            }
        }
        Timer::after(Duration::from_millis(500)).await;
    }
}

const STRIP_NODE: Node<'static> = Node {
    endpoints: &[
        root_endpoint!(eth),
        Endpoint::new(
            1,
            devices!(DEV_TYPE_EXTENDED_COLOR_LIGHT),
            clusters!(
                desc::DescHandler::CLUSTER,
                identify::CLUSTER,
                groups::GroupsHandler::CLUSTER,
                OnOffLogic::CLUSTER,
                LevelLogic::CLUSTER,
                ColorLogic::CLUSTER,
                SCENES_FULL_CLUSTER,
            ),
        ),
    ],
};

fn strip_data_model<'a, LH: LevelControlHooks, OH: OnOffHooks, CH: ColorControlHooks, R>(
    mut rand: impl Rng + Copy,
    on_off: &'a on_off::OnOffHandler<'a, OH, LH>,
    level_control: &'a level_control::LevelControlHandler<'a, LH, OH>,
    color_control: &'a color_control::ColorControlHandler<'a, CH, OH, LH>,
    scenes: ScenesHandler<'a, SCENES_CAPACITY, R>,
) -> impl DataModel + 'a
where
    R: SceneClusters + 'a,
{
    (
        STRIP_NODE,
        endpoints::EthSysHandlerBuilder::new()
            .netif_diag(&SysNetifs)
            .build(rand)
            .chain(
                |e, c| e == 1 && c == desc::DescHandler::CLUSTER.id,
                Async(desc::DescHandler::new(Dataver::new_rand(&mut rand)).adapt()),
            )
            .chain(
                |e, c| e == 1 && c == identify::CLUSTER.id,
                Async(IdentifyHandler::new(Dataver::new_rand(&mut rand)).adapt()),
            )
            .chain(
                |e, c| e == 1 && c == groups::GroupsHandler::CLUSTER.id,
                Async(groups::GroupsHandler::new(Dataver::new_rand(&mut rand)).adapt()),
            )
            .chain(
                |e, c| e == 1 && c == OnOffLogic::CLUSTER.id,
                Async(on_off::HandlerAdaptor(on_off)),
            )
            .chain(
                |e, c| e == 1 && c == LevelLogic::CLUSTER.id,
                Async(level_control::HandlerAdaptor(level_control)),
            )
            .chain(
                |e, c| e == 1 && c == ColorLogic::CLUSTER.id,
                Async(color_control::HandlerAdaptor(color_control)),
            )
            .chain(|e, c| e == 1 && c == SCENES_FULL_CLUSTER.id, scenes.adapt()),
    )
}

// Matter level 1..254 <-> WLED bri 1..255
fn to_bri(level: u8) -> u8 {
    ((level as u16 * 255 + 127) / 254).clamp(1, 255) as u8
}

fn to_level(bri: u8) -> u8 {
    ((bri as u16 * 254 + 127) / 255).clamp(1, 254) as u8
}

const SYNC_INTERVAL: Duration = Duration::from_secs(1);

pub struct ColorLogic<'a> {
    wled: &'static Wled,
    ready: &'a AtomicBool,
}

impl ColorControlHooks for ColorLogic<'_> {
    // Extended Color Light makes COLOR_TEMPERATURE mandatory; the cluster converts mireds
    // into a colour, so an RGB strip handles it via set_device_color too.
    const CLUSTER: Cluster<'static> = color_control_cluster::FULL_CLUSTER
        .with_features(
            color_control_cluster::Feature::HUE_AND_SATURATION.bits()
                | color_control_cluster::Feature::XY.bits()
                | color_control_cluster::Feature::COLOR_TEMPERATURE.bits(),
        )
        .with_attrs(with!(
            required;
            color_control_cluster::AttributeId::CurrentHue
            | color_control_cluster::AttributeId::CurrentSaturation
            | color_control_cluster::AttributeId::CurrentX
            | color_control_cluster::AttributeId::CurrentY
            | color_control_cluster::AttributeId::RemainingTime
            | color_control_cluster::AttributeId::ColorTemperatureMireds
            | color_control_cluster::AttributeId::ColorTempPhysicalMinMireds
            | color_control_cluster::AttributeId::ColorTempPhysicalMaxMireds
            | color_control_cluster::AttributeId::StartUpColorTemperatureMireds
        ))
        .with_cmds(with!(
            color_control_cluster::CommandId::MoveToHue
            | color_control_cluster::CommandId::MoveHue
            | color_control_cluster::CommandId::StepHue
            | color_control_cluster::CommandId::MoveToSaturation
            | color_control_cluster::CommandId::MoveSaturation
            | color_control_cluster::CommandId::StepSaturation
            | color_control_cluster::CommandId::MoveToHueAndSaturation
            | color_control_cluster::CommandId::MoveToColor
            | color_control_cluster::CommandId::MoveColor
            | color_control_cluster::CommandId::StepColor
            | color_control_cluster::CommandId::MoveToColorTemperature
            | color_control_cluster::CommandId::MoveColorTemperature
            | color_control_cluster::CommandId::StepColorTemperature
            | color_control_cluster::CommandId::StopMoveStep
        ));

    const COLOR_CAPABILITIES: ColorCapabilitiesBitmap = ColorCapabilitiesBitmap::from_bits_truncate(
        ColorCapabilitiesBitmap::HUE_SATURATION.bits()
            | ColorCapabilitiesBitmap::XY.bits()
            | ColorCapabilitiesBitmap::COLOR_TEMPERATURE.bits(),
    );

    // 6535K .. 2000K
    const COLOR_TEMP_PHYSICAL_MIN_MIREDS: u16 = 153;
    const COLOR_TEMP_PHYSICAL_MAX_MIREDS: u16 = 500;

    // White until the handler has persisted a colour
    const COLOR: Option<SetDeviceColor> = Some(SetDeviceColor::HueSaturation {
        enhanced_hue: 0,
        saturation: 0,
    });

    fn set_device_color(&self, target: SetDeviceColor) -> Result<(), ()> {
        // WLED applies its own gamma correction, so it gets display-encoded sRGB
        if self.ready.load(Ordering::SeqCst) {
            self.wled.set_rgb(target.to_rgb(RgbGamma::SRgb));
        }
        Ok(())
    }
}

pub struct LevelLogic<'a> {
    wled: &'static Wled,
    ready: &'a AtomicBool,
}

impl LevelControlHooks for LevelLogic<'_> {
    const MIN_LEVEL: u8 = 1;
    const MAX_LEVEL: u8 = 254;
    const FASTEST_RATE: u8 = 50;
    // None: turning on restores the previous level instead of a fixed one
    const ON_LEVEL: Option<u8> = None;
    const OPTIONS: OptionsBitmap = OptionsBitmap::EXECUTE_IF_OFF;
    const CURRENT_LEVEL: Option<u8> = Some(128);
    const CLUSTER: Cluster<'static> = LEVEL_CONTROL_FULL_CLUSTER
        .with_features(
            level_control::Feature::LIGHTING.bits() | level_control::Feature::ON_OFF.bits(),
        )
        .with_attrs(with!(
            required;
            AttributeId::CurrentLevel
            | AttributeId::RemainingTime
            | AttributeId::MinLevel
            | AttributeId::MaxLevel
            | AttributeId::OnOffTransitionTime
            | AttributeId::OnLevel
            | AttributeId::OnTransitionTime
            | AttributeId::OffTransitionTime
            | AttributeId::DefaultMoveRate
            | AttributeId::Options
            | AttributeId::StartUpCurrentLevel
        ))
        .with_cmds(with!(
            CommandId::MoveToLevel
                | CommandId::Move
                | CommandId::Step
                | CommandId::Stop
                | CommandId::MoveToLevelWithOnOff
                | CommandId::MoveWithOnOff
                | CommandId::StepWithOnOff
                | CommandId::StopWithOnOff
        ));

    fn set_device_level(&self, level: u8) -> Result<Option<u8>, ()> {
        if self.ready.load(Ordering::SeqCst) {
            self.wled.set_bri(to_bri(level));
        }
        Ok(Some(level))
    }

    async fn run<F: Fn(LevelMessage)>(&self, notify: F) {
        let mut last = None;
        loop {
            // While off, Matter keeps CurrentLevel at minimum (and a minimum level couples to
            // "off"), so WLED's remembered brightness is only mirrored while the strip is on
            if let Some(s) = self.wled.snapshot().filter(|s| s.on) {
                if last != Some(s.bri) {
                    notify(LevelMessage::Update(to_level(s.bri)));
                    last = Some(s.bri);
                }
            }
            Timer::after(SYNC_INTERVAL).await;
        }
    }
}

pub struct OnOffLogic<'a> {
    wled: &'static Wled,
    ready: &'a AtomicBool,
}

impl OnOffHooks for OnOffLogic<'_> {
    const CLUSTER: Cluster<'static> = on_off_cluster::FULL_CLUSTER
        .with_revision(6)
        .with_features(on_off_cluster::Feature::LIGHTING.bits())
        .with_attrs(with!(
            required;
            on_off_cluster::AttributeId::OnOff
            | on_off_cluster::AttributeId::GlobalSceneControl
            | on_off_cluster::AttributeId::OnTime
            | on_off_cluster::AttributeId::OffWaitTime
            | on_off_cluster::AttributeId::StartUpOnOff
        ))
        .with_cmds(with!(
            on_off_cluster::CommandId::Off
                | on_off_cluster::CommandId::On
                | on_off_cluster::CommandId::Toggle
                | on_off_cluster::CommandId::OffWithEffect
                | on_off_cluster::CommandId::OnWithRecallGlobalScene
                | on_off_cluster::CommandId::OnWithTimedOff
        ));

    fn set_on_off(&self, on: bool) {
        if self.ready.load(Ordering::SeqCst) {
            self.wled.set_on(on);
        }
    }

    async fn handle_off_with_effect(&self, _effect: on_off::EffectVariantEnum) {}

    async fn run<F: Fn(OnOffMessage)>(&self, notify: F) {
        let mut last = None;
        loop {
            if let Some(s) = self.wled.snapshot() {
                if last != Some(s.on) {
                    notify(OnOffMessage::Update(s.on));
                    last = Some(s.on);
                }
            }
            Timer::after(SYNC_INTERVAL).await;
        }
    }
}
