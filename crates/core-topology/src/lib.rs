use anyhow::{Context, Result};
use foundation::AppPaths;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use uuid::Uuid;

pub const DEFAULT_GRID_WIDTH: i32 = 3;
pub const DEFAULT_GRID_HEIGHT: i32 = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct GridPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceLayoutStatus {
    Online,
    Offline,
    Pending,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyDevice {
    pub device_id: Uuid,
    pub display_name: String,
    pub position: Option<GridPosition>,
    pub status: DeviceLayoutStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyLayout {
    pub version: u64,
    pub grid_width: i32,
    pub grid_height: i32,
    pub controller_device_id: Uuid,
    pub devices: Vec<TopologyDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyHotUpdate {
    pub previous_version: u64,
    pub next_version: u64,
    pub layout: TopologyLayout,
}

impl TopologyLayout {
    pub fn new(controller_device_id: Uuid, controller_name: impl Into<String>) -> Self {
        Self {
            version: 1,
            grid_width: DEFAULT_GRID_WIDTH,
            grid_height: DEFAULT_GRID_HEIGHT,
            controller_device_id,
            devices: vec![TopologyDevice {
                device_id: controller_device_id,
                display_name: controller_name.into(),
                position: Some(GridPosition {
                    x: DEFAULT_GRID_WIDTH / 2,
                    y: DEFAULT_GRID_HEIGHT / 2,
                }),
                status: DeviceLayoutStatus::Online,
            }],
        }
    }

    pub fn pending_devices(&self) -> Vec<&TopologyDevice> {
        self.devices
            .iter()
            .filter(|device| device.position.is_none())
            .collect()
    }

    pub fn device_at(&self, position: GridPosition) -> Option<&TopologyDevice> {
        self.devices
            .iter()
            .find(|device| device.position == Some(position))
    }

    pub fn device(&self, device_id: Uuid) -> Option<&TopologyDevice> {
        self.devices
            .iter()
            .find(|device| device.device_id == device_id)
    }

    pub fn add_pending_device(
        &mut self,
        device_id: Uuid,
        display_name: impl Into<String>,
    ) -> Result<()> {
        if self.devices.iter().any(|device| device.device_id == device_id) {
            anyhow::bail!("device already exists in topology");
        }

        self.devices.push(TopologyDevice {
            device_id,
            display_name: display_name.into(),
            position: None,
            status: DeviceLayoutStatus::Pending,
        });
        Ok(())
    }

    pub fn place_device(&mut self, device_id: Uuid, position: GridPosition) -> Result<()> {
        ensure_in_bounds(self.grid_width, self.grid_height, position)?;
        if self
            .devices
            .iter()
            .any(|device| device.device_id != device_id && device.position == Some(position))
        {
            anyhow::bail!("topology position is already occupied");
        }

        let device = self
            .devices
            .iter_mut()
            .find(|device| device.device_id == device_id)
            .context("device not found in topology")?;
        device.position = Some(position);
        if device.status == DeviceLayoutStatus::Pending {
            device.status = DeviceLayoutStatus::Online;
        }
        Ok(())
    }

    pub fn mark_offline(&mut self, device_id: Uuid) -> Result<()> {
        let device = self
            .devices
            .iter_mut()
            .find(|device| device.device_id == device_id)
            .context("device not found in topology")?;
        device.status = DeviceLayoutStatus::Offline;
        Ok(())
    }

    pub fn neighbor(&self, device_id: Uuid, direction: EdgeDirection) -> Option<&TopologyDevice> {
        let device = self.device(device_id)?;
        let position = device.position?;
        let target = match direction {
            EdgeDirection::Up => GridPosition {
                x: position.x,
                y: position.y - 1,
            },
            EdgeDirection::Down => GridPosition {
                x: position.x,
                y: position.y + 1,
            },
            EdgeDirection::Left => GridPosition {
                x: position.x - 1,
                y: position.y,
            },
            EdgeDirection::Right => GridPosition {
                x: position.x + 1,
                y: position.y,
            },
        };
        self.device_at(target)
    }

    pub fn validate(&self) -> Result<()> {
        ensure_in_bounds(self.grid_width, self.grid_height, GridPosition { x: 0, y: 0 })?;
        if self.grid_width <= 0 || self.grid_height <= 0 {
            anyhow::bail!("topology grid dimensions must be positive");
        }

        let mut occupied = HashMap::new();
        for device in &self.devices {
            if let Some(position) = device.position {
                ensure_in_bounds(self.grid_width, self.grid_height, position)?;
                if let Some(other) = occupied.insert(position, device.device_id) {
                    anyhow::bail!(
                        "topology devices overlap at ({}, {}): {} and {}",
                        position.x,
                        position.y,
                        other,
                        device.device_id
                    );
                }
            }
        }

        if !self
            .devices
            .iter()
            .any(|device| device.device_id == self.controller_device_id && device.position.is_some())
        {
            anyhow::bail!("controller device must be placed");
        }

        self.ensure_connected()
    }

    fn ensure_connected(&self) -> Result<()> {
        let placed: HashMap<GridPosition, Uuid> = self
            .devices
            .iter()
            .filter_map(|device| device.position.map(|position| (position, device.device_id)))
            .collect();
        if placed.is_empty() {
            return Ok(());
        }

        let Some(start) = self
            .device(self.controller_device_id)
            .and_then(|device| device.position)
        else {
            anyhow::bail!("controller device must be placed");
        };

        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([start]);
        while let Some(position) = queue.pop_front() {
            if !visited.insert(position) {
                continue;
            }

            for neighbor in neighbors(position) {
                if placed.contains_key(&neighbor) && !visited.contains(&neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }

        if visited.len() != placed.len() {
            anyhow::bail!("topology contains isolated placed devices");
        }

        Ok(())
    }
}

pub fn load_or_create_topology(
    paths: &AppPaths,
    controller_device_id: Uuid,
    controller_name: &str,
) -> Result<TopologyLayout> {
    paths.ensure_layout()?;
    let path = paths.topology_file();
    if path.exists() {
        let raw = fs::read_to_string(&path).context("read topology file")?;
        let layout: TopologyLayout = serde_json::from_str(&raw).context("parse topology file")?;
        let layout = migrate_layout_to_default_grid(layout);
        layout.validate()?;
        save_topology(paths, &layout)?;
        return Ok(layout);
    }

    let layout = TopologyLayout::new(controller_device_id, controller_name);
    save_topology(paths, &layout)?;
    Ok(layout)
}

fn migrate_layout_to_default_grid(mut layout: TopologyLayout) -> TopologyLayout {
    if layout.grid_width == DEFAULT_GRID_WIDTH && layout.grid_height == DEFAULT_GRID_HEIGHT {
        return layout;
    }

    layout.grid_width = DEFAULT_GRID_WIDTH;
    layout.grid_height = DEFAULT_GRID_HEIGHT;
    let controller_center = GridPosition {
        x: DEFAULT_GRID_WIDTH / 2,
        y: DEFAULT_GRID_HEIGHT / 2,
    };

    let mut occupied = HashSet::from([controller_center]);
    for device in &mut layout.devices {
        if device.device_id == layout.controller_device_id {
            device.position = Some(controller_center);
            if device.status == DeviceLayoutStatus::Pending {
                device.status = DeviceLayoutStatus::Online;
            }
            continue;
        }

        let keep_position = device.position.is_some_and(|position| {
            position.x >= 0
                && position.y >= 0
                && position.x < DEFAULT_GRID_WIDTH
                && position.y < DEFAULT_GRID_HEIGHT
                && !occupied.contains(&position)
        });
        if keep_position {
            occupied.insert(device.position.expect("position checked above"));
        } else {
            device.position = None;
            device.status = DeviceLayoutStatus::Pending;
        }
    }

    if layout.validate().is_err() {
        for device in &mut layout.devices {
            if device.device_id != layout.controller_device_id {
                device.position = None;
                device.status = DeviceLayoutStatus::Pending;
            }
        }
    }

    layout
}

pub fn save_topology(paths: &AppPaths, layout: &TopologyLayout) -> Result<()> {
    paths.ensure_layout()?;
    layout.validate()?;
    let raw = serde_json::to_string_pretty(layout).context("serialize topology")?;
    fs::write(paths.topology_file(), raw).context("write topology file")
}

pub fn apply_hot_update(
    current: &TopologyLayout,
    mut next: TopologyLayout,
) -> Result<TopologyHotUpdate> {
    next.version = current.version + 1;
    next.validate()?;
    Ok(TopologyHotUpdate {
        previous_version: current.version,
        next_version: next.version,
        layout: next,
    })
}

fn ensure_in_bounds(width: i32, height: i32, position: GridPosition) -> Result<()> {
    if position.x < 0 || position.y < 0 || position.x >= width || position.y >= height {
        anyhow::bail!(
            "topology position out of bounds: ({}, {}) for {}x{} grid",
            position.x,
            position.y,
            width,
            height
        );
    }
    Ok(())
}

fn neighbors(position: GridPosition) -> [GridPosition; 4] {
    [
        GridPosition {
            x: position.x,
            y: position.y - 1,
        },
        GridPosition {
            x: position.x,
            y: position.y + 1,
        },
        GridPosition {
            x: position.x - 1,
            y: position.y,
        },
        GridPosition {
            x: position.x + 1,
            y: position.y,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths(name: &str) -> AppPaths {
        let root = std::env::temp_dir()
            .join("deskflow-plus-topology-tests")
            .join(name);
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }
        AppPaths::from_root(root)
    }

    #[test]
    fn default_topology_places_controller_at_grid_center() {
        let controller = Uuid::new_v4();
        let layout = TopologyLayout::new(controller, "controller");

        assert_eq!(layout.grid_width, DEFAULT_GRID_WIDTH);
        assert_eq!(layout.grid_height, DEFAULT_GRID_HEIGHT);
        assert_eq!(
            layout.device(controller).expect("controller").position,
            Some(GridPosition { x: 1, y: 1 })
        );
        layout.validate().expect("valid default topology");
    }

    #[test]
    fn newly_paired_device_can_stay_pending_until_placed() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout = TopologyLayout::new(controller, "controller");

        layout
            .add_pending_device(client, "client")
            .expect("add pending");

        assert_eq!(layout.pending_devices().len(), 1);
        layout.validate().expect("pending does not invalidate layout");
    }

    #[test]
    fn placing_device_next_to_controller_creates_neighbor_relationship() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout = TopologyLayout::new(controller, "controller");
        layout.add_pending_device(client, "client").expect("add pending");
        layout
            .place_device(client, GridPosition { x: 2, y: 1 })
            .expect("place client");

        let neighbor = layout
            .neighbor(controller, EdgeDirection::Right)
            .expect("right neighbor");
        assert_eq!(neighbor.device_id, client);
        layout.validate().expect("valid adjacent layout");
    }

    #[test]
    fn validation_rejects_overlapping_devices() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout = TopologyLayout::new(controller, "controller");
        layout.add_pending_device(client, "client").expect("add pending");
        layout.devices.push(TopologyDevice {
            device_id: client,
            display_name: "duplicate".into(),
            position: Some(GridPosition { x: 1, y: 1 }),
            status: DeviceLayoutStatus::Online,
        });

        let error = layout.validate().expect_err("overlap should fail");
        assert!(error.to_string().contains("overlap"));
    }

    #[test]
    fn validation_rejects_isolated_placed_devices() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout = TopologyLayout::new(controller, "controller");
        layout.devices.push(TopologyDevice {
            device_id: client,
            display_name: "client".into(),
            position: Some(GridPosition { x: 0, y: 0 }),
            status: DeviceLayoutStatus::Online,
        });

        let error = layout.validate().expect_err("isolated layout should fail");
        assert!(error.to_string().contains("isolated"));
    }

    #[test]
    fn validation_rejects_out_of_bounds_positions() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout = TopologyLayout::new(controller, "controller");
        layout.devices.push(TopologyDevice {
            device_id: client,
            display_name: "client".into(),
            position: Some(GridPosition { x: 99, y: 99 }),
            status: DeviceLayoutStatus::Online,
        });

        let error = layout.validate().expect_err("out of bounds should fail");
        assert!(error.to_string().contains("out of bounds"));
    }

    #[test]
    fn topology_persists_and_reloads() {
        let paths = test_paths("persist");
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout =
            load_or_create_topology(&paths, controller, "controller").expect("create topology");
        layout.add_pending_device(client, "client").expect("add pending");
        layout
            .place_device(client, GridPosition { x: 2, y: 1 })
            .expect("place client");
        save_topology(&paths, &layout).expect("save topology");

        let reloaded =
            load_or_create_topology(&paths, controller, "controller").expect("reload topology");
        assert_eq!(reloaded, layout);
    }

    #[test]
    fn offline_device_keeps_layout_slot() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut layout = TopologyLayout::new(controller, "controller");
        layout.add_pending_device(client, "client").expect("add pending");
        layout
            .place_device(client, GridPosition { x: 2, y: 1 })
            .expect("place client");

        layout.mark_offline(client).expect("mark offline");

        let offline = layout.device(client).expect("offline device");
        assert_eq!(offline.status, DeviceLayoutStatus::Offline);
        assert_eq!(offline.position, Some(GridPosition { x: 2, y: 1 }));
        layout.validate().expect("offline slot remains valid");
    }

    #[test]
    fn hot_update_validates_and_increments_version() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut current = TopologyLayout::new(controller, "controller");
        current.add_pending_device(client, "client").expect("add pending");

        let mut next = current.clone();
        next.place_device(client, GridPosition { x: 2, y: 1 })
            .expect("place client");
        let update = apply_hot_update(&current, next).expect("apply hot update");

        assert_eq!(update.previous_version, 1);
        assert_eq!(update.next_version, 2);
        assert_eq!(
            update.layout.neighbor(controller, EdgeDirection::Right).map(|device| device.device_id),
            Some(client)
        );
    }

    #[test]
    fn hot_update_completes_under_reasonable_bound() {
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let mut current = TopologyLayout::new(controller, "controller");
        current.add_pending_device(client, "client").expect("add pending");
        let mut next = current.clone();
        next.place_device(client, GridPosition { x: 2, y: 1 })
            .expect("place client");

        let started = std::time::Instant::now();
        let update = apply_hot_update(&current, next).expect("apply hot update");
        let elapsed = started.elapsed();

        println!("topology hot update elapsed: {elapsed:?}");
        assert_eq!(update.next_version, 2);
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "topology hot update took {elapsed:?}"
        );
    }

    #[test]
    fn persisted_legacy_grid_is_migrated_to_three_by_three() {
        let paths = test_paths("legacy-grid-migration");
        let controller = Uuid::new_v4();
        let client = Uuid::new_v4();
        let legacy = TopologyLayout {
            version: 1,
            grid_width: 5,
            grid_height: 3,
            controller_device_id: controller,
            devices: vec![
                TopologyDevice {
                    device_id: controller,
                    display_name: "controller".into(),
                    position: Some(GridPosition { x: 2, y: 1 }),
                    status: DeviceLayoutStatus::Online,
                },
                TopologyDevice {
                    device_id: client,
                    display_name: "client".into(),
                    position: Some(GridPosition { x: 3, y: 1 }),
                    status: DeviceLayoutStatus::Online,
                },
            ],
        };
        paths.ensure_layout().expect("create topology directories");
        fs::write(
            paths.topology_file(),
            serde_json::to_string_pretty(&legacy).expect("serialize legacy"),
        )
        .expect("write legacy topology");

        let migrated =
            load_or_create_topology(&paths, controller, "controller").expect("migrate topology");

        assert_eq!(migrated.grid_width, 3);
        assert_eq!(migrated.grid_height, 3);
        assert_eq!(
            migrated.device(controller).expect("controller").position,
            Some(GridPosition { x: 1, y: 1 })
        );
        assert_eq!(migrated.device(client).expect("client").position, None);
        assert_eq!(
            migrated.device(client).expect("client").status,
            DeviceLayoutStatus::Pending
        );
    }
}
