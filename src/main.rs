#![feature(conservative_impl_trait, proc_macro, unboxed_closures)]
#![windows_subsystem = "windows"]

extern crate chrono;
extern crate rexiv2;
extern crate location_history;
extern crate geo;
extern crate cogset;
extern crate walkdir;
extern crate gtk;
extern crate gdk;
extern crate gdk_pixbuf;
extern crate odds;
#[macro_use]
extern crate relm;
extern crate relm_attributes;
#[macro_use]
extern crate relm_derive;
extern crate futures;
extern crate futures_cpupool;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

use rexiv2::Metadata;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use odds::vec::VecExt;
use location_history::{Locations, LocationsExt};
use cogset::{Dbscan, BruteScan};
use walkdir::WalkDir;
use gtk::{BoxExt, CellLayoutExt, ContainerExt, FileChooserDialog, FileChooserExt, DialogExt,
          Inhibit, Menu, MenuBar, MenuItem, MenuItemExt, MenuShellExt, OrientableExt,
          ScrolledWindowExt, TreeView, Viewport, WidgetExt, WindowExt};
use gtk::Orientation::{Vertical, Horizontal};
use gdk::prelude::ContextExt;
use relm::{Relm, Update, Widget};
use relm_attributes::widget;
use futures::{Future, lazy};
use futures::future::ok;
use futures::Async::{Ready, NotReady};
use futures_cpupool::{CpuPool, CpuFuture};
use serde_json::{Value, Error, Map};
use geo::Bbox;
use geo::contains::Contains;

mod photo;

use photo::{Photo, TimePhoto};
use self::Msg::*;
use self::ViewMsg::*;
use self::MenuMsg::*;

#[derive(Deserialize, Debug)]
struct Geo {
    address: Map<String, Value>,
    #[serde(deserialize_with = "parse_bbox")]
    boundingbox: Bbox<f64>,
}

fn parse_bbox<'de, D>(de: D) -> Result<Bbox<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let deser_result: serde_json::Value = try!(serde::Deserialize::deserialize(de));
    match deser_result {
        serde_json::Value::Array(ref s) => {
            Ok(Bbox {
                xmin: s[2].as_str().unwrap().parse::<f64>().unwrap(),
                xmax: s[3].as_str().unwrap().parse::<f64>().unwrap(),
                ymin: s[0].as_str().unwrap().parse::<f64>().unwrap(),
                ymax: s[1].as_str().unwrap().parse::<f64>().unwrap(),
            })
        }
        _ => Err(serde::de::Error::custom("Unexpected value")),
    }
}

// The messages that can be sent to the update function.
#[derive(Msg)]
enum MenuMsg {
    SelectFile,
    SelectFolder,
    MenuAbout,
    MenuQuit,
}

#[derive(Clone)]
struct MyMenuBar {
    bar: MenuBar,
}

/// all the events are handled in Win
impl Update for MyMenuBar {
    type Model = ();
    type ModelParam = ();
    type Msg = MenuMsg;

    fn model(_: &Relm<Self>, _: ()) {}

    fn update(&mut self, _event: MenuMsg) {}
}

impl Widget for MyMenuBar {
    type Root = MenuBar;

    fn root(&self) -> Self::Root {
        self.bar.clone()
    }

    fn view(relm: &Relm<Self>, _model: Self::Model) -> Self {
        let menu_file = Menu::new();
        let menu_help = Menu::new();
        let menu_bar = MenuBar::new();

        let file = MenuItem::new_with_label("File");
        let quit = MenuItem::new_with_label("Quit");
        let folder_item = MenuItem::new_with_label("Import photos");
        let file_item = MenuItem::new_with_label("Import LocationHistory");

        let help = MenuItem::new_with_label("Help");
        let about = MenuItem::new_with_label("About");

        connect!(relm, quit, connect_activate(_), MenuQuit);
        connect!(relm, folder_item, connect_activate(_), SelectFolder);
        connect!(relm, file_item, connect_activate(_), SelectFile);
        connect!(relm, about, connect_activate(_), MenuAbout);

        menu_file.append(&folder_item);
        menu_file.append(&file_item);
        menu_file.append(&quit);
        file.set_submenu(Some(&menu_file));


        menu_help.append(&about);
        help.set_submenu(&menu_help);

        menu_bar.append(&file);
        menu_bar.append(&help);
        menu_bar.show_all();

        MyMenuBar { bar: menu_bar }
    }
}

#[derive(Clone)]
struct MyViewPort {
    view: Viewport,
    tree: TreeView,
}

#[derive(Msg)]
pub enum ViewMsg {
    UpdateView(gtk::TreeStore),
}

impl Update for MyViewPort {
    type Model = ();
    type ModelParam = ();
    type Msg = ViewMsg;

    fn model(_: &Relm<Self>, _: ()) {}

    fn update(&mut self, event: ViewMsg) {
        match event {
            UpdateView(model) => {
                self.tree.set_model(&model);
            }
        }
    }
}

impl Widget for MyViewPort {
    type Root = Viewport;

    fn root(&self) -> Self::Root {
        self.view.clone()
    }

    fn view(_relm: &Relm<Self>, _model: Self::Model) -> Self {
        // TODO: change column names and labels
        let view = Viewport::new(None, None);
        let tree = TreeView::new();
        let name_column = gtk::TreeViewColumn::new();
        let name_column_cell = gtk::CellRendererText::new();
        name_column.set_title("Name");
        name_column.pack_start(&name_column_cell, true);

        let start_column = gtk::TreeViewColumn::new();
        let start_column_cell = gtk::CellRendererText::new();
        start_column.set_title("Start date");
        start_column.pack_start(&start_column_cell, true);

        let end_column = gtk::TreeViewColumn::new();
        let end_column_cell = gtk::CellRendererText::new();
        end_column.set_title("End date");
        end_column.pack_start(&end_column_cell, true);

        tree.append_column(&name_column);
        //tree.append_column(&start_column);
        //tree.append_column(&end_column);

        name_column.add_attribute(&name_column_cell, "text", 0);
        start_column.add_attribute(&start_column_cell, "text", 1);
        end_column.add_attribute(&end_column_cell, "text", 2);

        view.add(&tree);

        view.show_all();

        MyViewPort { view, tree }
    }
}

#[derive(Clone)]
pub struct Model {
    relm: Relm<Win>,
    locations: Locations,
    photos: Vec<Photo>,
    pool: Arc<CpuPool>,
    queue: Arc<Mutex<Vec<CpuFuture<String, ()>>>>,
}

#[derive(Msg)]
pub enum Msg {
    JsonDialog,
    FolderDialog,
    AboutDialog,
    Quit,
    GeoLookup(f64, f64),
    Processed(String),
    Process,
}

#[widget]
impl Widget for Win {
    fn init_view(&mut self) {
        self.map.connect_draw(move |widget, context| {
            let width = widget.get_allocated_width() as f64;
            let height = widget.get_allocated_height() as f64;
            let pix = gdk_pixbuf::Pixbuf::new_from_file("src/map.png").unwrap();
            let width_scale = width / pix.get_width() as f64;
            let height_scale = height / pix.get_height() as f64;
            let scale = if width_scale < height_scale {
                width_scale
            } else {
                height_scale
            };
            context.scale(scale, scale);
            context.set_source_pixbuf(&pix, 0f64, 0f64);
            context.paint();
            return Inhibit(false);
        });
        let relm = self.model.relm.clone();
        gtk::timeout_add_seconds(1, move || {
            relm.stream().emit(Process);
            gtk::Continue(true)
        });
    }

    // The initial model.
    fn model(relm: &Relm<Self>, _: ()) -> Model {
        Model {
            relm: relm.clone(),
            locations: Vec::new(),
            photos: Vec::new(),
            pool: Arc::new(CpuPool::new_num_cpus()),
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // Update the model according to the message received.
    fn update(&mut self, event: Msg) {
        match event {
            JsonDialog => {
                if let Some(x) = self.json_dialog() {
                    self.model.locations = self.load_json(x);
                    self.update_locations();
                };
            }
            FolderDialog => {
                if let Some(x) = self.folder_dialog() {
                    self.model.photos = self.load_photos(x);
                    self.update_locations();
                    self.view.emit(UpdateView(self.cluster_location()));
                    self.cluster_time();
                };
            }
            AboutDialog => self.about_dialog(),
            Quit => gtk::main_quit(),
            GeoLookup(lat, lon) => {
                let some_future = lazy(move || {
                    let req = format!("http://locationiq.org/v1/reverse.php?format=json&zoom=13&key={:?}&lat={}&lon={:?}",
                                    "", lat, lon);
                    let mut resp = reqwest::get(&req).unwrap();
                    let mut content = String::new();
                    resp.read_to_string(&mut content);
                    ok(content)
                });
                let mut queue = self.model.queue.lock().unwrap();
                queue.push(self.model.pool.spawn(some_future));
            },
            Processed(result) => {
                if let Ok(v) = serde_json::from_str::<Geo>(&result){
                    println!("{:?}\n{:?}\n", v.address["country"], v.boundingbox);
                    for photo in self.model.photos.iter_mut() {
                        if let Some(location) = photo.location {
                            if v.boundingbox.contains(&location){
                                photo.location_name = Some(v.address["country"].as_str().unwrap().to_owned());
                            }
                        } 
                    }
                    self.view.emit(UpdateView(self.cluster_location()));
                }
            },
            Process => {
                let mut queue = self.model.queue.lock().unwrap();
                let current = queue.pop();
                if let Some(mut x) = current { 
                    match x.poll() {
                        Ok(Ready(result)) => {
                            println!("{}", result);
                            self.model.relm.stream().emit(Processed(result));
                        },
                        Ok(NotReady) => {
                            println!("not ready");
                            queue.push(x);
                        },
                        Err(_) => {},
                    }
                }
            }
        }
    }

    view! {
        #[name="root"]
        gtk::Window {
            title: "Photos-rs",
            gtk::Box {
                // Set the orientation property of the Box.
                orientation: Vertical,
                MyMenuBar {
                    SelectFile => JsonDialog,
                    SelectFolder => FolderDialog,
                    MenuAbout => AboutDialog,
                    MenuQuit => Quit,
                },
                gtk::Box {
                    packing: {
                                expand: true,
                    },
                    orientation: Horizontal,
                    #[name="map"]
                    gtk::DrawingArea {
                        packing: {
                                expand: true,
                        },
                    },
                    gtk::Box{
                        orientation: Vertical,
                        gtk::Label {
                            text: "Clusters",
                        },
                        gtk::ScrolledWindow {
                            property_hscrollbar_policy: gtk::PolicyType::Never,
                            packing: {
                                expand: true,
                                fill: true,
                            },
                            #[name="view"]
                            MyViewPort,
                        },
                    },
                },
            },
            delete_event(_, _) => (Quit, Inhibit(false)),
        }
    }
}

impl Win {
    fn json_dialog(&self) -> Option<PathBuf> {
        let dialog = FileChooserDialog::new::<gtk::Window>(
            Some("Import File"),
            Some(&self.root()),
            gtk::FileChooserAction::Open,
        );
        let filter = gtk::FileFilter::new();
        filter.set_name("json");
        filter.add_pattern("*.json");
        dialog.add_filter(&filter);
        dialog.add_button("Ok", gtk::ResponseType::Ok.into());
        dialog.add_button("Cancel", gtk::ResponseType::Cancel.into());
        let response_ok: i32 = gtk::ResponseType::Ok.into();
        if dialog.run() == response_ok {
            let path = dialog.get_filename();
            dialog.destroy();
            return path;
        }
        dialog.destroy();
        None
    }

    fn folder_dialog(&self) -> Option<PathBuf> {
        let dialog = FileChooserDialog::new::<gtk::Window>(
            Some("Import File"),
            Some(&self.root()),
            gtk::FileChooserAction::SelectFolder,
        );
        dialog.add_button("Ok", gtk::ResponseType::Ok.into());
        dialog.add_button("Cancel", gtk::ResponseType::Cancel.into());
        let response_ok: i32 = gtk::ResponseType::Ok.into();
        if dialog.run() == response_ok {
            let path = dialog.get_filename();
            dialog.destroy();
            return path;
        }
        dialog.destroy();
        None
    }

    fn about_dialog(&self) {
        let dialog = gtk::AboutDialog::new();
        dialog.set_transient_for(&self.root());
        dialog.set_modal(true);
        dialog.set_authors(&["Eric Trombly"]);
        dialog.set_program_name("Photos-rs");
        dialog.set_comments("Photo tagger");
        if let Ok(logo) = gdk_pixbuf::Pixbuf::new_from_file("Antu_map-globe.ico") {
            dialog.set_logo(Some(&logo));
        };
        dialog.run();
        dialog.destroy();
    }

    fn load_json(&self, path: PathBuf) -> Locations {
        // read json file
        let mut contents = String::new();
        File::open(path)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        location_history::deserialize(&contents).filter_outliers()
    }

    fn load_photos(&self, path: PathBuf) -> Vec<Photo> {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        println!("Scanning photos");
        let files = WalkDir::new(path).into_iter().filter_map(|e| e.ok());
        let files = files.filter(|x| Metadata::new_from_path(x.path()).is_ok());
        files.map(|x| Photo::new(x.path().to_path_buf())).collect()
    }

    fn update_locations(&mut self) {
        for photo in self.model.photos.iter_mut() {
            if photo.location == None {
                if let Some(time) = photo.time {
                    println!("  Date: {:?}", time);
                    if let Some(closest) = self.model.locations.find_closest(time) {
                        println!(
                            "  closest timestamp: {:?} long: {} lat: {} accuracy: {}",
                            closest.timestamp,
                            closest.longitude,
                            closest.latitude,
                            closest.accuracy
                        );
                        photo.set_location(closest);
                    }
                }
            }
        }
    }

    fn cluster_location(&self) -> gtk::TreeStore {
        let scanner = BruteScan::new(&self.model.photos);
        let mut dbscan = Dbscan::new(scanner, 1000.0, 3);
        let clusters = dbscan.by_ref().collect::<Vec<_>>();
        let model = gtk::TreeStore::new(&[gtk::Type::String, gtk::Type::String, gtk::Type::String]);
        for cluster in clusters {
            let top = model.append(None);
            if let Some(x) = cluster.iter().find(|&&x| self.model.photos[x].location_name.is_some()){
                model.set(&top, &[0], &[self.model.photos[*x].location_name.as_ref().unwrap()]);
            } else if let Some(point) = self.model.photos[cluster[0]].location {
                model.set(&top, &[0], &[&format!("{}, {}",point.y(), point.x())]);
                self.model.relm.stream().emit(GeoLookup(point.y(), point.x()));
            }
            for photo in cluster {
                let entries = model.append(&top);
                model.set(
                    &entries,
                    &[0],
                    &[&format!("{:?} ", self.model.photos[photo].path)],
                );
            }
        }
        model
    }

    fn cluster_time(&self) {
        let timephotos = self.model
            .photos
            .iter()
            .map(|x| TimePhoto(x))
            .collect::<Vec<_>>();
        let timescanner = BruteScan::new(&timephotos);
        let mut timedbscan = Dbscan::new(timescanner, 600.0, 10);
        let timeclusters = timedbscan.by_ref().collect::<Vec<_>>();
        for cluster in timeclusters {
            for photo in cluster {
                print!("{:?} ", timephotos[photo].0.path);
            }
        }
    }
}

fn main() {
    Win::run(()).unwrap();
}
