use serde::{de::Error, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Config {
    pub pepe: PepeSettings,
    pub tests: Vec<HttpTest>,
}

impl Config {

    #[allow(dead_code)]
    pub fn from_yaml(yaml_str: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml_str)
    }

    pub fn from_yaml_file(file_path: &str) -> Result<Self, serde_yaml::Error> {
        let yaml_str = std::fs::read_to_string(file_path);
        match yaml_str {
            Ok(yaml_str) => serde_yaml::from_str(&yaml_str),
            Err(e) => Err(serde_yaml::Error::custom(e)),
        }
    }

    #[allow(dead_code)]
    pub fn build_execution_plan(&self) -> HashMap<String, Vec<String>> {
        let tests = &self.tests;
        let mut dependency_map: HashMap<String, Vec<String>> = HashMap::new();

        for test in tests {
            if let Some(dep) = &test.depends_on {
                dependency_map
                    .entry(dep.clone())
                    .or_default()
                    .push(test.id.clone());
            } else {
                dependency_map
                    .entry("ROOT".to_string())
                    .or_default()
                    .push(test.id.clone());
            }
        }

        dependency_map
    }

    #[allow(dead_code)]
    pub fn display_execution_plan(&self) {
        let tests = &self.tests;
        let mut dependency_map: HashMap<&str, Vec<&str>> = HashMap::new();

        // Build dependency tree
        for test in tests {
            if let Some(dep) = &test.depends_on {
                dependency_map
                    .entry(dep.as_str())
                    .or_default()
                    .push(test.id.as_str());
            } else {
                dependency_map
                    .entry("ROOT")
                    .or_default()
                    .push(test.id.as_str());
            }
        }

        // Recursive function to print the tree properly
        fn print_plan(node: &str, map: &HashMap<&str, Vec<&str>>, depth: usize, is_last: bool) {
            let prefix = if depth == 0 {
                "".to_string()
            } else if is_last {
                "  ".repeat(depth - 1) + "└── "
            } else {
                "  ".repeat(depth - 1) + "├── "
            };

            if node != "ROOT" {
                println!("{}{}", prefix, node);
            }

            if let Some(children) = map.get(node) {
                let len = children.len();
                for (i, child) in children.iter().enumerate() {
                    print_plan(child, map, depth + 1, i == len - 1);
                }
            }
        }

        println!("\nExecution Plan:");
        if let Some(root_tests) = dependency_map.get("ROOT") {
            let len = root_tests.len();
            for (i, root) in root_tests.iter().enumerate() {
                print_plan(root, &dependency_map, 0, i == len - 1);
            }
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PepeSettings {
    pub concurrency: u32,
    pub timeout: u32,
    pub number: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct HttpTest {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub depends_on: Option<String>,
    pub curl: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_parse_valid_yaml() {
        let yaml_str = indoc! {"
            pepe:
              concurrency: 10
              timeout: 10
              number: 500

            tests:
              - id: \"auth_request\"
                name: \"Get Auth Token\"
                url: \"https://api.example.com/auth\"
                method: \"POST\"
                headers:
                  Content-Type: \"application/json\"

              - id: \"fetch_user\"
                name: \"Fetch User Info\"
                url: \"https://api.example.com/user\"
                method: \"GET\"
                headers:
                  Authorization: \"Bearer {{ token }}\"
                depends_on: \"auth_request\"
        "};

        let config = Config::from_yaml(yaml_str).expect("Failed to parse YAML");

        assert_eq!(config.pepe.concurrency, 10);
        assert_eq!(config.pepe.timeout, 10);
        assert_eq!(config.pepe.number, 500);
        assert_eq!(config.tests.len(), 2);
        assert_eq!(config.tests[0].id, "auth_request");
        assert_eq!(config.tests[1].depends_on.as_deref(), Some("auth_request"));
    }

    #[test]
    fn test_execution_plan() {
        let yaml_str = indoc! {"
            pepe:
              concurrency: 10
              timeout: 10
              number: 500

            tests:
              - id: \"step1\"
                name: \"Step 1\"
                url: \"https://api.example.com/step1\"
                method: \"GET\"
                headers:
                  User-Agent: \"Pepe\"

              - id: \"step2\"
                name: \"Step 2\"
                url: \"https://api.example.com/step2\"
                method: \"GET\"
                headers:
                  Authorization: \"Bearer {{ token }}\"
                depends_on: \"step1\"

              - id: \"step3\"
                name: \"Step 3\"
                url: \"https://api.example.com/step3\"
                method: \"GET\"
                headers:
                  Authorization: \"Bearer {{ token }}\"
                depends_on: \"step2\"
        "};

        let config = Config::from_yaml(yaml_str).expect("Failed to parse YAML");
        let plan = config.build_execution_plan();
        config.display_execution_plan();

        assert_eq!(plan.get("ROOT").unwrap(), &vec!["step1"]);
        assert_eq!(plan.get("step1").unwrap(), &vec!["step2"]);
        assert_eq!(plan.get("step2").unwrap(), &vec!["step3"]);
        assert!(plan.get("step3").is_none());
    }

    #[test]
    fn test_no_dependencies() {
        let yaml_str = indoc! {"
            pepe:
              concurrency: 5
              timeout: 5
              number: 100

            tests:
              - id: \"task1\"
                name: \"Task 1\"
                url: \"https://api.example.com/task1\"
                method: \"POST\"
                headers:
                  Content-Type: \"application/json\"

              - id: \"task2\"
                name: \"Task 2\"
                url: \"https://api.example.com/task2\"
                method: \"POST\"
                headers:
                  Content-Type: \"application/json\"
        "};

        let config = Config::from_yaml(yaml_str).expect("Failed to parse YAML");
        let plan = config.build_execution_plan();

        assert_eq!(plan.get("ROOT").unwrap().len(), 2);
        assert!(plan.get("task1").is_none());
        assert!(plan.get("task2").is_none());
    }

    #[test]
    fn test_load_from_file() {
        let file_path = "src/config/examples/config.yaml";
        let config = Config::from_yaml_file(file_path).expect("Failed to parse YAML");

        let plan = config.build_execution_plan();
        assert_eq!(plan.get("ROOT").unwrap().len(), 2);
        config.display_execution_plan();

        assert_eq!(config.pepe.concurrency, 10);
        assert_eq!(config.pepe.timeout, 10);
        assert_eq!(config.pepe.number, 500);
    }
}
