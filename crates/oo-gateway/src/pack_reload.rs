//! Validate a replacement once and pin one immutable Pack catalog per request.
use crate::pack_catalog::{PackCatalog, PackError};
use std::{fs, path::{Path,PathBuf}, sync::{Arc,Mutex}, time::SystemTime};

#[derive(PartialEq,Eq)]
struct Stamp {
    path:PathBuf,
    length:u64,
    modified:SystemTime,
    #[cfg(unix)]
    identity:(u64,u64,i64,i64),
}
fn stamps(directory:&Path)->Result<Vec<Stamp>,PackError> {
    let fail=||PackError{reason:"pack_reload"};
    let mut files=fs::read_dir(directory).map_err(|_|fail())?
        .map(|entry|entry.map(|e|e.path()).map_err(|_|fail())).collect::<Result<Vec<_>,_>>()?;
    files.sort();
    files.into_iter().map(|path| {
        let meta=fs::symlink_metadata(&path).map_err(|_|fail())?;
        if !meta.is_file() || meta.file_type().is_symlink() {return Err(fail());}
        #[cfg(unix)]
        let identity={use std::os::unix::fs::MetadataExt;
            (meta.dev(),meta.ino(),meta.ctime(),meta.ctime_nsec())};
        Ok(Stamp{path,length:meta.len(),modified:meta.modified().map_err(|_|fail())?,
            #[cfg(unix)] identity})
    }).collect()
}

pub(crate) struct ReloadingPacks {
    directory:PathBuf,
    ceilings:oo_builder::RolePackCeilings,
    current:Mutex<(Vec<Stamp>,Arc<PackCatalog>)>,
}
impl ReloadingPacks {
    pub fn load(directory:PathBuf,ceilings:oo_builder::RolePackCeilings)->Result<Self,PackError> {
        let files=stamps(&directory)?;
        let catalog=PackCatalog::load_dir_with_ceilings(&directory,&ceilings)?;
        if files!=stamps(&directory)? {return Err(PackError{reason:"pack_changed"});}
        Ok(Self{directory,ceilings,current:Mutex::new((files,Arc::new(catalog)))})
    }
    pub fn snapshot(&self)->Result<Arc<PackCatalog>,PackError> {
        let mut current=self.current.lock().map_err(|_|PackError{reason:"pack_lock"})?;
        let files=stamps(&self.directory)?;
        if cfg!(unix) && files==current.0 {return Ok(Arc::clone(&current.1));}
        let catalog=PackCatalog::load_dir_with_ceilings(&self.directory,&self.ceilings)?;
        if files!=stamps(&self.directory)? {return Err(PackError{reason:"pack_changed"});}
        *current=(files,Arc::new(catalog));
        Ok(Arc::clone(&current.1))
    }
}
