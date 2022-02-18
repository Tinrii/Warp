use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use warp_data::DataObject;
use warp_module::Module;
use warp_pocket_dimension::query::{Comparator, QueryBuilder};
use warp_pocket_dimension::{error::Error, PocketDimension};

// MemoryCache instance will hold a map of both module and dataobject "in memory".
// There is little functionality to it for testing purchase outside of `PocketDimension` interface
// Note: This `MemoryCache` is a cheap and dirty way of testing currently.
//      Such code here should not really be used in production
#[derive(Default)]
pub struct MemoryCache(HashMap<Module, Vec<DataObject>>);

impl MemoryCache {
    pub fn flush(&mut self) {
        let _ = self.0.drain().collect::<Vec<_>>();
    }
}

impl PocketDimension for MemoryCache {
    fn add_data<T: Serialize>(&mut self, dimension: Module, data: T) -> Result<DataObject, Error> {
        //TODO: Determine size of payload for `DataObject::size`
        let mut object = DataObject::new(&dimension, data)?;
        if let Some(val) = self.0.get_mut(&dimension) {
            let version = val.len();
            object.version = version as i32;
            val.push(object.clone());
        } else {
            self.0.insert(dimension, vec![object.clone()]);
        }
        Ok(object)
    }

    fn get_data(
        &self,
        dimension: Module,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<DataObject>, Error> {
        let data = self.0.get(&dimension).ok_or(Error::Other)?;
        match query {
            Some(query) => execute(data, query),
            None => Ok(data.clone()),
        }
    }

    fn size(&self, dimension: Module, query: Option<&QueryBuilder>) -> Result<i64, Error> {
        self.get_data(dimension, query)
            .map(|data| data.iter().map(|i| i.size as i64).sum())
    }

    fn count(&self, dimension: Module, query: Option<&QueryBuilder>) -> Result<i64, Error> {
        self.get_data(dimension, query)
            .map(|data| data.len() as i64)
    }

    fn empty(&mut self, dimension: Module) -> Result<Vec<DataObject>, Error> {
        self.0
            .get_mut(&dimension)
            .map(|val| val.drain(..).collect())
            .ok_or(Error::Other)
    }
}

//Cheap "filter"
fn execute(data: &Vec<DataObject>, query: &QueryBuilder) -> Result<Vec<DataObject>, Error> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = data.payload::<Value>()?;
        if !object.is_object() {
            continue;
        }
        let object = object.as_object().ok_or(Error::Other)?;
        for (key, val) in query.r#where.iter() {
            if let Some(result) = object.get(key) {
                if val == result {
                    list.push(data.clone());
                }
            }
        }
        for (comp, key, val) in query.comparator.iter() {
            match comp {
                Comparator::Eq => {
                    if let Some(result) = object.get(key) {
                        if result == val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Ne => {
                    if let Some(result) = object.get(key) {
                        if result != val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Gte => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result >= val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Gt => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result > val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Lte => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result <= val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Lt => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result < val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
            }
        }

        if let Some(limit) = query.limit {
            if list.len() > limit {
                list = list.drain(..limit).collect();
            }
        }
    }
    Ok(list)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SomeData {
    pub name: String,
    pub age: i64,
}

impl Default for SomeData {
    fn default() -> Self {
        Self {
            name: String::from("John Doe"),
            age: 21,
        }
    }
}

impl SomeData {
    pub fn set_name<S: AsRef<str>>(&mut self, name: S) {
        self.name = name.as_ref().to_string();
    }
    pub fn set_age(&mut self, age: i64) {
        self.age = age
    }
}

fn generate_data(amount: i64) -> Vec<SomeData> {
    let mut list = Vec::new();
    for i in 0..amount {
        list.push({
            let mut data = SomeData::default();
            data.set_name(&format!("Test Subject {i}"));
            data.set_age(18 + i);
            data
        });
    }
    list
}

#[test]
fn if_count_eq_five() -> Result<(), Error> {
    let mut memory = MemoryCache::default();

    let list = generate_data(100);

    for data in list {
        memory.add_data(Module::Accounts, data)?;
    }

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Gte, "age", 19)?;
    query.limit(5);

    let count = memory.count(Module::Accounts, Some(&query))?;

    assert_eq!(count, 5);

    Ok(())
}

#[test]
fn data_test() -> Result<(), Error> {
    let mut memory = MemoryCache::default();

    let list = generate_data(100);

    for data in list {
        memory.add_data(Module::Accounts, data)?;
    }

    let mut query = QueryBuilder::default();
    query.r#where("age", 21)?;

    let data = memory.get_data(Module::Accounts, Some(&query))?;

    assert_eq!(data.get(0).unwrap().payload::<SomeData>().unwrap().age, 21);

    Ok(())
}