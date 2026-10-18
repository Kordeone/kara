// SPDX-License-Identifier: MPL-2.0
//! Static StatusNotifierItem + D-Bus menu on libcosmic's activation connection.
use crate::storage::APP_ID;
use cosmic::iced::{
    Subscription,
    futures::{SinkExt, StreamExt},
};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use zbus::{
    Connection, Proxy,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const ITEM: &str = "/StatusNotifierItem";
const MENU: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ICON: &str = "view-list-symbolic";
type Props = HashMap<String, OwnedValue>;
type Layout = (i32, Props, Vec<OwnedValue>);
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Open(Option<String>),
    Quit,
    Availability(Result<(), String>),
}
struct Shared {
    events: mpsc::UnboundedSender<Event>,
    token: Mutex<Option<(Instant, String)>>,
}
impl Shared {
    fn open(&self) {
        let token = self
            .token
            .lock()
            .unwrap()
            .take()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(10))
            .map(|(_, token)| token);
        let _ = self.events.send(Event::Open(token));
    }
    fn event(&self, id: i32, event: &str) {
        if event == "clicked" {
            match id {
                1 => self.open(),
                2 => {
                    self.token.lock().unwrap().take();
                    let _ = self.events.send(Event::Quit);
                }
                _ => {}
            }
        }
    }
}
struct Item(Arc<Shared>);
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, _x: i32, _y: i32) {
        self.0.open();
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        self.0.open();
    }
    fn context_menu(&self, _x: i32, _y: i32) {}
    fn scroll(&self, _delta: i32, _orientation: &str) {}
    // COSMIC sends this before Activate and before a D-Bus menu click.
    fn provide_xdg_activation_token(&self, token: String) {
        *self.0.token.lock().unwrap() = (!token.is_empty()).then(|| (Instant::now(), token));
    }
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }
    #[zbus(property)]
    fn id(&self) -> &str {
        APP_ID
    }
    #[zbus(property)]
    fn title(&self) -> &str {
        "Kara"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }
    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ICON
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        vec![]
    }
    #[zbus(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Pixmaps {
        vec![]
    }
    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        vec![]
    }
    #[zbus(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        (ICON.into(), vec![], "Kara".into(), "Tasks and lists".into())
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU).unwrap()
    }
    #[zbus(signal)]
    async fn new_icon(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_status(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        status: &str,
    ) -> zbus::Result<()>;
}
struct Menu(Arc<Shared>);
fn props(id: i32, names: &[&str]) -> zbus::fdo::Result<Props> {
    let label = match id {
        0 => "",
        1 => "Open Kara",
        2 => "Quit",
        _ => return Err(zbus::fdo::Error::InvalidArgs("Unknown menu ID".into())),
    };
    let mut props = Props::from([
        ("label".into(), Value::from(label).try_into().unwrap()),
        ("enabled".into(), true.into()),
        ("visible".into(), true.into()),
    ]);
    if id == 0 {
        props.insert(
            "children-display".into(),
            Value::from("submenu").try_into().unwrap(),
        );
    }
    if !names.is_empty() {
        props.retain(|name, _| names.contains(&name.as_str()));
    }
    Ok(props)
}
fn layout(id: i32, depth: i32, names: &[&str]) -> zbus::fdo::Result<Layout> {
    let props = props(id, names)?;
    let children = if id == 0 && depth != 0 {
        [1, 2]
            .into_iter()
            .map(|id| {
                let child = layout(id, 0, names)?;
                OwnedValue::try_from(Value::from(child))
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            })
            .collect::<zbus::fdo::Result<Vec<_>>>()?
    } else {
        vec![]
    };
    Ok((id, props, children))
}
#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }
    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        vec![]
    }
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<&str>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        Ok((1, layout(parent_id, recursion_depth, &property_names)?))
    }
    fn get_group_properties(&self, ids: Vec<i32>, property_names: Vec<&str>) -> Vec<(i32, Props)> {
        let ids = if ids.is_empty() { vec![0, 1, 2] } else { ids };
        ids.into_iter()
            .filter_map(|id| props(id, &property_names).ok().map(|p| (id, p)))
            .collect()
    }
    fn get_property(&self, id: i32, name: &str) -> zbus::fdo::Result<OwnedValue> {
        props(id, &[])?
            .remove(name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("Unknown menu property".into()))
    }
    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) {
        self.0.event(id, event_id);
    }
    fn event_group(&self, events: Vec<(i32, String, Value<'_>, u32)>) -> Vec<i32> {
        let mut errors = vec![];
        for (id, event, _, _) in events {
            if (0..=2).contains(&id) {
                self.0.event(id, &event);
            } else {
                errors.push(id);
            }
        }
        errors
    }
    fn about_to_show(&self, id: i32) -> zbus::fdo::Result<bool> {
        props(id, &[]).map(|_| false)
    }
    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (
            vec![],
            ids.into_iter().filter(|id| !(0..=2).contains(id)).collect(),
        )
    }
    #[zbus(signal)]
    async fn layout_updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn items_properties_updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        updated: Vec<(i32, Props)>,
        removed: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}
async fn serve(conn: &Connection, events: mpsc::UnboundedSender<Event>) -> zbus::Result<()> {
    let shared = Arc::new(Shared {
        events,
        token: Mutex::new(None),
    });
    if !conn.object_server().at(ITEM, Item(shared.clone())).await?
        || !conn.object_server().at(MENU, Menu(shared)).await?
    {
        return Err(zbus::Error::Failure(
            "Tray interfaces are already registered".into(),
        ));
    }
    Ok(())
}
// COSMIC's watcher reports HostRegistered=true unconditionally. Observe only
// property calls addressed to our own item, then check those consumers' unique
// bus names. A panel disappearing cannot leave us trusting a surviving watcher.
async fn consumer_stream(conn: &Connection) -> zbus::Result<zbus::MessageStream> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::MethodCall)
        .interface("org.freedesktop.DBus.Properties")?
        .path(ITEM)?
        .build();
    zbus::MessageStream::for_match_rule(rule, conn, Some(32)).await
}
fn record_consumer(message: &zbus::Message, consumers: &mut HashSet<String>) {
    let header = message.header();
    let body = message.body();
    let interface = match header.member().map(|member| member.as_str()) {
        Some("Get") => body
            .deserialize::<(&str, &str)>()
            .ok()
            .map(|(interface, _)| interface),
        Some("GetAll") => body.deserialize::<&str>().ok(),
        _ => None,
    };
    if interface == Some("org.kde.StatusNotifierItem")
        && let Some(sender) = header.sender()
    {
        consumers.insert(sender.to_string());
    }
}
/// Verify both registration and a host. Use a unique destination so a watcher
/// restart cannot combine properties from different owners in a single check.
async fn check(
    conn: &Connection,
    registered_owner: &mut Option<String>,
    consumers: &mut HashSet<String>,
) -> Result<(), String> {
    let dbus = zbus::fdo::DBusProxy::new(conn)
        .await
        .map_err(|e| e.to_string())?;
    let owner = dbus
        .get_name_owner(WATCHER.try_into().unwrap())
        .await
        .map_err(|e| e.to_string())?
        .to_string();
    let watcher = Proxy::new(conn, owner.as_str(), WATCHER_PATH, WATCHER)
        .await
        .map_err(|e| e.to_string())?;
    // Use explicit Get calls to avoid zbus property caching masking host loss.
    let properties = Proxy::new(
        conn,
        owner.as_str(),
        WATCHER_PATH,
        "org.freedesktop.DBus.Properties",
    )
    .await
    .map_err(|e| e.to_string())?;
    let items: OwnedValue = properties
        .call("Get", &(WATCHER, "RegisteredStatusNotifierItems"))
        .await
        .map_err(|e| e.to_string())?;
    let items = Vec::<String>::try_from(items).map_err(|e| e.to_string())?;
    let ours = format!(
        "{}{ITEM}",
        conn.unique_name().ok_or("D-Bus has no unique name")?
    );
    if registered_owner.as_deref() != Some(&owner) || !items.contains(&ours) {
        watcher
            .call::<_, _, ()>("RegisterStatusNotifierItem", &(ITEM,))
            .await
            .map_err(|e| e.to_string())?;
        *registered_owner = Some(owner.clone());
    }
    let host: OwnedValue = properties
        .call("Get", &(WATCHER, "IsStatusNotifierHostRegistered"))
        .await
        .map_err(|e| e.to_string())?;
    if !bool::try_from(host).map_err(|e| e.to_string())? {
        return Err("No tray host is registered".into());
    }
    let items: OwnedValue = properties
        .call("Get", &(WATCHER, "RegisteredStatusNotifierItems"))
        .await
        .map_err(|e| e.to_string())?;
    if !Vec::<String>::try_from(items)
        .map_err(|e| e.to_string())?
        .contains(&ours)
    {
        return Err("The tray watcher did not retain Kara's registration".into());
    }
    let mut live = HashSet::new();
    for consumer in consumers.iter() {
        if dbus
            .name_has_owner(
                consumer
                    .as_str()
                    .try_into()
                    .map_err(|e: zbus::names::Error| e.to_string())?,
            )
            .await
            .map_err(|e| e.to_string())?
        {
            live.insert(consumer.clone());
        }
    }
    *consumers = live;
    if consumers.is_empty() {
        return Err("No connected tray host has read Kara's icon".into());
    }
    Ok(())
}
struct Input(Connection);
impl Hash for Input {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.unique_name().hash(state);
    }
}
pub fn subscription(conn: Connection) -> Subscription<Event> {
    Subscription::run_with(Input(conn), |input| {
        let conn = input.0.clone();
        cosmic::iced::stream::channel(
            32,
            move |mut output: cosmic::iced::futures::channel::mpsc::Sender<Event>| async move {
                let (tx, mut rx) = mpsc::unbounded_channel();
                let mut reads = match consumer_stream(&conn).await {
                    Ok(reads) => reads,
                    Err(error) => {
                        let _ = output
                            .send(Event::Availability(Err(error.to_string())))
                            .await;
                        std::future::pending::<()>().await;
                        return;
                    }
                };
                let mut consumers = HashSet::new();
                if let Err(error) = serve(&conn, tx).await {
                    let _ = output
                        .send(Event::Availability(Err(error.to_string())))
                        .await;
                    std::future::pending::<()>().await;
                }
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut owner = None;
                let mut previous = None;
                loop {
                    let event = tokio::select! {
                        Some(event) = rx.recv() => event,
                        message = reads.next() => {
                            match message {
                                Some(Ok(message)) => { record_consumer(&message,&mut consumers); continue; }
                                _ => {
                                    let _ = output.send(Event::Availability(Err("Tray connection was lost".into()))).await;
                                    std::future::pending::<()>().await;
                                    return;
                                }
                            }
                        }
                        _ = interval.tick() => {
                            let result = tokio::time::timeout(Duration::from_secs(2),check(&conn,&mut owner,&mut consumers)).await
                                .unwrap_or_else(|_|Err("Tray watcher timed out".into()));
                            if previous.as_ref() == Some(&result) { continue; }
                            previous = Some(result.clone()); Event::Availability(result)
                        }
                    };
                    if output.send(event).await.is_err() {
                        break;
                    }
                }
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    #[test]
    fn menu_layout_is_static_filters_properties_and_rejects_unknown_ids() {
        let root = layout(0, -1, &[]).unwrap();
        assert_eq!(root.2.len(), 2);
        assert_eq!(layout(0, 0, &[]).unwrap().2.len(), 0);
        assert_eq!(props(1, &["label"]).unwrap().len(), 1);
        assert_eq!(
            String::try_from(props(1, &[]).unwrap().remove("label").unwrap()).unwrap(),
            "Open Kara"
        );
        assert_eq!(
            String::try_from(props(2, &[]).unwrap().remove("label").unwrap()).unwrap(),
            "Quit"
        );
        assert!(layout(99, -1, &[]).is_err());
        use zbus::zvariant::Type;
        assert_eq!(Layout::SIGNATURE.to_string(), "(ia{sv}av)");
    }
    #[test]
    fn activation_tokens_expire_and_are_consumed_once() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            events: tx,
            token: Mutex::new(None),
        });
        let item = Item(shared.clone());
        item.provide_xdg_activation_token("fresh".into());
        item.activate(0, 0);
        assert_eq!(rx.try_recv().unwrap(), Event::Open(Some("fresh".into())));
        item.activate(0, 0);
        assert_eq!(rx.try_recv().unwrap(), Event::Open(None));
        *shared.token.lock().unwrap() =
            Some((Instant::now() - Duration::from_secs(20), "expired".into()));
        item.activate(0, 0);
        assert_eq!(rx.try_recv().unwrap(), Event::Open(None));
        item.provide_xdg_activation_token("discard-on-quit".into());
        shared.event(2, "opened");
        assert!(rx.try_recv().is_err());
        shared.event(2, "clicked");
        assert_eq!(rx.try_recv().unwrap(), Event::Quit);
        assert!(shared.token.lock().unwrap().is_none());
    }
    struct Watcher {
        items: Arc<Mutex<Vec<String>>>,
        host: Arc<AtomicBool>,
        accept: Arc<AtomicBool>,
    }
    #[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
    impl Watcher {
        fn register_status_notifier_item(
            &self,
            service: &str,
            #[zbus(header)] header: zbus::message::Header<'_>,
        ) -> zbus::fdo::Result<()> {
            if !self.accept.load(Ordering::SeqCst) {
                return Err(zbus::fdo::Error::Failed("Registration rejected".into()));
            }
            let item = format!("{}{}", header.sender().unwrap(), service);
            let mut items = self.items.lock().unwrap();
            if !items.contains(&item) {
                items.push(item);
            }
            Ok(())
        }
        #[zbus(property)]
        fn registered_status_notifier_items(&self) -> Vec<String> {
            self.items.lock().unwrap().clone()
        }
        #[zbus(property)]
        fn is_status_notifier_host_registered(&self) -> bool {
            self.host.load(Ordering::SeqCst)
        }
    }
    #[test]
    #[ignore = "requires an isolated session bus; run the command in report.txt"]
    fn dbus_protocol_registration_recovery_and_single_instance_transport() {
        assert_eq!(
            std::env::var("KARA_PRIVATE_TRAY_TEST").as_deref(),
            Ok("1"),
            "Run this test only on the isolated bus described in report.txt"
        );
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let server = Connection::session().await.unwrap();
            let mut reads = consumer_stream(&server).await.unwrap();
            let mut consumers = HashSet::new();
            let (tx, mut events) = mpsc::unbounded_channel();
            serve(&server, tx).await.unwrap();
            let mut activation = cosmic::DbusActivation::new();
            let mut activations = activation.rx();
            let activation_path = format!("/{}", APP_ID.replace('.', "/"));
            server
                .object_server()
                .at(activation_path.as_str(), activation)
                .await
                .unwrap();
            server.request_name(APP_ID).await.unwrap();
            let client = Connection::session().await.unwrap();
            let item = Proxy::new(&client, APP_ID, ITEM, "org.kde.StatusNotifierItem")
                .await
                .unwrap();
            assert_eq!(item.get_property::<String>("IconName").await.unwrap(), ICON);
            assert!(!item.get_property::<bool>("ItemIsMenu").await.unwrap());
            assert_eq!(
                item.get_property::<OwnedObjectPath>("Menu")
                    .await
                    .unwrap()
                    .as_str(),
                MENU
            );
            item.call::<_, _, ()>("ProvideXdgActivationToken", &("tray-token",))
                .await
                .unwrap();
            item.call::<_, _, ()>("Activate", &(0i32, 0i32))
                .await
                .unwrap();
            assert_eq!(
                events.recv().await.unwrap(),
                Event::Open(Some("tray-token".into()))
            );
            let menu = Proxy::new(&client, APP_ID, MENU, "com.canonical.dbusmenu")
                .await
                .unwrap();
            let (revision, root): (u32, Layout) = menu
                .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
                .await
                .unwrap();
            assert_eq!(revision, 1);
            assert_eq!(root.2.len(), 2);
            item.call::<_, _, ()>("ProvideXdgActivationToken", &("menu-token",))
                .await
                .unwrap();
            menu.call::<_, _, ()>("Event", &(1i32, "clicked", Value::from(0i32), 0u32))
                .await
                .unwrap();
            assert_eq!(
                events.recv().await.unwrap(),
                Event::Open(Some("menu-token".into()))
            );
            menu.call::<_, _, ()>("Event", &(2i32, "clicked", Value::from(0i32), 0u32))
                .await
                .unwrap();
            assert_eq!(events.recv().await.unwrap(), Event::Quit);
            let proxy = Proxy::new(
                &client,
                APP_ID,
                activation_path.as_str(),
                "org.freedesktop.DbusActivation",
            )
            .await
            .unwrap();
            proxy
                .call::<_, _, ()>(
                    "ActivateAction",
                    &(
                        "open",
                        vec!["launch-token"],
                        HashMap::<String, Value<'_>>::new(),
                    ),
                )
                .await
                .unwrap();
            use cosmic::iced::futures::StreamExt;
            let message = activations.next().await.unwrap();
            match message.msg {
                cosmic::dbus_activation::Details::ActivateAction { action, args } => {
                    assert_eq!(action, "open");
                    assert_eq!(args, vec!["launch-token"]);
                }
                _ => panic!("Expected existing-instance activation"),
            }
            use cosmic::iced::futures::FutureExt;
            while let Some(Some(Ok(message))) = reads.next().now_or_never() {
                record_consumer(&message, &mut consumers);
            }
            assert!(consumers.contains(client.unique_name().unwrap().as_str()));
            let mut owner = None;
            assert!(check(&server, &mut owner, &mut consumers).await.is_err());
            let host = Arc::new(AtomicBool::new(true));
            let accept = Arc::new(AtomicBool::new(true));
            let items = Arc::new(Mutex::new(vec![]));
            let watcher = Connection::session().await.unwrap();
            watcher
                .object_server()
                .at(
                    WATCHER_PATH,
                    Watcher {
                        items: items.clone(),
                        host: host.clone(),
                        accept: accept.clone(),
                    },
                )
                .await
                .unwrap();
            watcher.request_name(WATCHER).await.unwrap();
            check(&server, &mut owner, &mut consumers).await.unwrap();
            assert_eq!(items.lock().unwrap().len(), 1);
            host.store(false, Ordering::SeqCst);
            assert!(check(&server, &mut owner, &mut consumers).await.is_err());
            host.store(true, Ordering::SeqCst);
            check(&server, &mut owner, &mut consumers).await.unwrap();
            items.lock().unwrap().clear();
            check(&server, &mut owner, &mut consumers).await.unwrap();
            assert_eq!(items.lock().unwrap().len(), 1);
            items.lock().unwrap().clear();
            accept.store(false, Ordering::SeqCst);
            assert!(check(&server, &mut owner, &mut consumers).await.is_err());
            accept.store(true, Ordering::SeqCst);
            check(&server, &mut owner, &mut consumers).await.unwrap();
            let previous_owner = owner.clone();
            watcher.release_name(WATCHER).await.unwrap();
            assert!(check(&server, &mut owner, &mut consumers).await.is_err());
            let restarted = Connection::session().await.unwrap();
            items.lock().unwrap().clear();
            restarted
                .object_server()
                .at(
                    WATCHER_PATH,
                    Watcher {
                        items: items.clone(),
                        host,
                        accept,
                    },
                )
                .await
                .unwrap();
            restarted.request_name(WATCHER).await.unwrap();
            check(&server, &mut owner, &mut consumers).await.unwrap();
            assert_ne!(owner, previous_owner);
            assert_eq!(items.lock().unwrap().len(), 1);
            client.close().await.unwrap();
            assert!(check(&server, &mut owner, &mut consumers).await.is_err());
            let restored = Connection::session().await.unwrap();
            let restored_item = Proxy::new(&restored, APP_ID, ITEM, "org.kde.StatusNotifierItem")
                .await
                .unwrap();
            restored_item
                .get_property::<String>("IconName")
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while !consumers.contains(restored.unique_name().unwrap().as_str()) {
                    record_consumer(&reads.next().await.unwrap().unwrap(), &mut consumers);
                }
            })
            .await
            .unwrap();
            check(&server, &mut owner, &mut consumers).await.unwrap();
            server.close().await.unwrap();
            let dbus = zbus::fdo::DBusProxy::new(&restored).await.unwrap();
            assert!(
                dbus.get_name_owner(APP_ID.try_into().unwrap())
                    .await
                    .is_err()
            );
        });
    }
}
