/*
 * VegaFusion
 * Copyright (C) 2022 VegaFusion Technologies LLC
 *
 * This program is distributed under multiple licenses.
 * Please consult the license documentation provided alongside
 * this program the details of the active license.
 */
#[cfg(test)]
mod tests {
    use crate::crate_dir;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use vegafusion_core::data::table::VegaFusionTable;
    use vegafusion_core::error::VegaFusionError;
    use vegafusion_core::proto::gen::tasks::Variable;
    use vegafusion_rt_datafusion::data::dataset::VegaFusionDataset;
    use vegafusion_rt_datafusion::data::table::VegaFusionTableUtils;
    use vegafusion_rt_datafusion::task_graph::runtime::TaskGraphRuntime;

    #[tokio::test]
    async fn test_pre_transform_dataset() {
        // Load spec
        let spec_path = format!("{}/tests/specs/vegalite/histogram.vg.json", crate_dir());
        let spec_str = fs::read_to_string(spec_path).unwrap();

        // Initialize task graph runtime
        let runtime = TaskGraphRuntime::new(Some(16), Some(1024_i32.pow(3) as usize));

        let (values, warnings) = runtime
            .pre_transform_values(
                &spec_str,
                &[(Variable::new_data("source_0"), vec![])],
                "UTC",
                &None,
                Default::default(),
            )
            .await
            .unwrap();

        // Check there are no warnings
        assert!(warnings.is_empty());

        // Check single returned dataset
        assert_eq!(values.len(), 1);

        let dataset = values[0].as_table().cloned().unwrap();

        let expected = "\
+----------------------------+--------------------------------+---------+
| bin_maxbins_10_IMDB Rating | bin_maxbins_10_IMDB Rating_end | __count |
+----------------------------+--------------------------------+---------+
| 6                          | 7                              | 985     |
| 3                          | 4                              | 100     |
| 7                          | 8                              | 741     |
| 5                          | 6                              | 633     |
| 8                          | 9                              | 204     |
| 2                          | 3                              | 43      |
| 4                          | 5                              | 273     |
| 9                          | 10                             | 4       |
| 1                          | 2                              | 5       |
+----------------------------+--------------------------------+---------+";
        assert_eq!(dataset.pretty_format(None).unwrap(), expected);
    }

    #[tokio::test]
    async fn test_pre_transform_validate() {
        // Load spec
        let spec_path = format!("{}/tests/specs/vegalite/area_density.vg.json", crate_dir());
        let spec_str = fs::read_to_string(spec_path).unwrap();

        // Initialize task graph runtime
        let runtime = TaskGraphRuntime::new(Some(16), Some(1024_i32.pow(3) as usize));

        // Check existent but unsupported dataset name
        let result = runtime
            .pre_transform_values(
                &spec_str,
                &[(Variable::new_data("source_0"), vec![])],
                "UTC",
                &None,
                Default::default(),
            )
            .await;

        if let Err(VegaFusionError::PreTransformError(err, _)) = result {
            assert_eq!(
                err,
                "Requested variable (Variable { name: \"source_0\", namespace: Data }, [])\n \
                requires transforms or signal expressions that are not yet supported"
            )
        } else {
            panic!("Expected PreTransformError");
        }

        // Check non-existent dataset name
        let result = runtime
            .pre_transform_values(
                &spec_str,
                &[(Variable::new_data("bogus_0"), vec![])],
                "UTC",
                &None,
                Default::default(),
            )
            .await;

        if let Err(VegaFusionError::PreTransformError(err, _)) = result {
            assert_eq!(err, "No dataset named bogus_0 with scope []")
        } else {
            panic!("Expected PreTransformError");
        }
    }

    #[tokio::test]
    async fn test_pre_transform_with_dots_in_fieldname() {
        // Load spec
        let spec_path = format!(
            "{}/tests/specs/inline_datasets/period_in_field_name.vg.json",
            crate_dir()
        );
        let spec_str = fs::read_to_string(spec_path).unwrap();

        // Initialize task graph runtime
        let runtime = TaskGraphRuntime::new(Some(16), Some(1024_i32.pow(3) as usize));

        let source_0 = VegaFusionTable::from_json(
            &json!([{"normal": 1, "a.b": 2}, {"normal": 1, "a.b": 4}]),
            16,
        )
        .unwrap();

        let source_0_dataset =
            VegaFusionDataset::from_table_ipc_bytes(&source_0.to_ipc_bytes().unwrap()).unwrap();
        let inline_datasets: HashMap<_, _> = vec![("source_0".to_string(), source_0_dataset)]
            .into_iter()
            .collect();

        let (values, warnings) = runtime
            .pre_transform_values(
                &spec_str,
                &[(Variable::new_data("source_0"), vec![])],
                "UTC",
                &None,
                inline_datasets,
            )
            .await
            .unwrap();

        // Check there are no warnings
        assert!(warnings.is_empty());

        // Check single returned dataset
        assert_eq!(values.len(), 1);

        let dataset = values[0].as_table().cloned().unwrap();
        println!("{}", dataset.pretty_format(None).unwrap());

        let expected = "\
+--------+-----+
| normal | a.b |
+--------+-----+
| 1      | 2   |
| 1      | 4   |
+--------+-----+";
        assert_eq!(dataset.pretty_format(None).unwrap(), expected);
    }

    #[tokio::test]
    async fn test_pre_transform_with_empty_store() {
        // Load spec
        let spec_path = format!(
            "{}/tests/specs/pre_transform/empty_store_array.vg.json",
            crate_dir()
        );
        let spec_str = fs::read_to_string(spec_path).unwrap();

        // Initialize task graph runtime
        let runtime = TaskGraphRuntime::new(Some(16), Some(1024_i32.pow(3) as usize));

        let (values, warnings) = runtime
            .pre_transform_values(
                &spec_str,
                &[(Variable::new_data("data_3"), vec![])],
                "UTC",
                &None,
                Default::default(),
            )
            .await
            .unwrap();

        // Check there are no warnings
        assert!(warnings.is_empty());

        // Check single returned dataset
        assert_eq!(values.len(), 1);

        let dataset = values[0].as_table().cloned().unwrap();
        let first_row = dataset.head(1);

        println!("{}", first_row.pretty_format(None).unwrap());

        let expected = "\
+-------+-----------+------+-----------------+
| yield | variety   | year | site            |
+-------+-----------+------+-----------------+
| 27    | Manchuria | 1931 | University Farm |
+-------+-----------+------+-----------------+";
        assert_eq!(first_row.pretty_format(None).unwrap(), expected);
    }

    #[tokio::test]
    async fn test_pre_transform_with_datetime_strings_in_store() {
        // Load spec
        let spec_path = format!(
            "{}/tests/specs/pre_transform/datetime_strings_in_selection_stores.vg.json",
            crate_dir()
        );
        let spec_str = fs::read_to_string(spec_path).unwrap();

        // Initialize task graph runtime
        let runtime = TaskGraphRuntime::new(Some(16), Some(1024_i32.pow(3) as usize));

        let (values, warnings) = runtime
            .pre_transform_values(
                &spec_str,
                &[
                    (Variable::new_data("click_selected"), vec![]),
                    (Variable::new_data("drag_selected"), vec![]),
                ],
                "UTC",
                &None,
                Default::default(),
            )
            .await
            .unwrap();

        // Check there are no warnings
        assert!(warnings.is_empty());

        // Check two returned datasets
        assert_eq!(values.len(), 2);

        // Check click_selected
        let click_selected = values[0].as_table().cloned().unwrap();

        println!("{}", click_selected.pretty_format(None).unwrap());

        let expected = "\
+---------------------+---------------------+---------+---------+---------------+-------------+
| yearmonth_date      | yearmonth_date_end  | weather | __count | __count_start | __count_end |
+---------------------+---------------------+---------+---------+---------------+-------------+
| 2013-11-01T00:00:00 | 2013-12-01T00:00:00 | rain    | 15      | 12            | 27          |
| 2014-01-01T00:00:00 | 2014-02-01T00:00:00 | sun     | 16      | 0             | 16          |
+---------------------+---------------------+---------+---------+---------------+-------------+";
        assert_eq!(click_selected.pretty_format(None).unwrap(), expected);

        // Check drag_selected
        let drag_selected = values[1].as_table().cloned().unwrap();
        println!("{}", drag_selected.pretty_format(None).unwrap());

        let expected = "\
+---------------------+---------------------+---------+---------+---------------+-------------+
| yearmonth_date      | yearmonth_date_end  | weather | __count | __count_start | __count_end |
+---------------------+---------------------+---------+---------+---------------+-------------+
| 2013-11-01T00:00:00 | 2013-12-01T00:00:00 | rain    | 15      | 12            | 27          |
| 2013-11-01T00:00:00 | 2013-12-01T00:00:00 | drizzle | 1       | 29            | 30          |
| 2013-11-01T00:00:00 | 2013-12-01T00:00:00 | sun     | 12      | 0             | 12          |
| 2013-11-01T00:00:00 | 2013-12-01T00:00:00 | fog     | 2       | 27            | 29          |
| 2013-12-01T00:00:00 | 2014-01-01T00:00:00 | rain    | 13      | 18            | 31          |
| 2013-12-01T00:00:00 | 2014-01-01T00:00:00 | sun     | 17      | 0             | 17          |
| 2013-12-01T00:00:00 | 2014-01-01T00:00:00 | snow    | 1       | 17            | 18          |
| 2014-01-01T00:00:00 | 2014-02-01T00:00:00 | sun     | 16      | 0             | 16          |
| 2014-01-01T00:00:00 | 2014-02-01T00:00:00 | rain    | 13      | 16            | 29          |
| 2014-01-01T00:00:00 | 2014-02-01T00:00:00 | fog     | 2       | 29            | 31          |
+---------------------+---------------------+---------+---------+---------------+-------------+";
        assert_eq!(drag_selected.pretty_format(None).unwrap(), expected);
    }
}

fn crate_dir() -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .display()
        .to_string()
}
