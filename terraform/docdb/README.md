# `docdb` module

Creates a DocumentDB cluster with auto-scaled read replicas.

<!-- BEGIN_TF_DOCS -->

## Requirements

| Name | Version |
|------|---------|
| <a name="requirement_terraform"></a> [terraform](#requirement\_terraform) | ~> 1.0 |
| <a name="requirement_aws"></a> [aws](#requirement\_aws) | ~> 5.7 |
| <a name="requirement_random"></a> [random](#requirement\_random) | ~> 3.5 |
## Providers

| Name | Version |
|------|---------|
| <a name="provider_aws"></a> [aws](#provider\_aws) | ~> 5.7 |
| <a name="provider_random"></a> [random](#provider\_random) | ~> 3.5 |
## Modules

| Name | Source | Version |
|------|--------|---------|
| <a name="module_this"></a> [this](#module\_this) | app.terraform.io/wallet-connect/label/null | 0.3.2 |

## Inputs
| Name | Description | Type | Default | Required |
|------|-------------|------|---------|:--------:|
| <a name="input_allow_ingress_from_self"></a> [allow\_ingress\_from\_self](#input\_allow\_ingress\_from\_self) | Adds the Document DB security group itself as a source for ingress rules. Useful when this security group will be shared with applications. |  <pre lang="json">bool</pre> |  <pre lang="json">false</pre> |  no |
| <a name="input_allowed_cidr_blocks"></a> [allowed\_cidr\_blocks](#input\_allowed\_cidr\_blocks) | List of CIDR blocks to be allowed to connect to the DocumentDB cluster |  <pre lang="json">list(string)</pre> |  <pre lang="json">[]</pre> |  no |
| <a name="input_allowed_egress_cidr_blocks"></a> [allowed\_egress\_cidr\_blocks](#input\_allowed\_egress\_cidr\_blocks) | List of CIDR blocks to be allowed to send traffic outside of the DocumentDB cluster |  <pre lang="json">list(string)</pre> |  <pre lang="json">[<br>  "0.0.0.0/0"<br>]</pre> |  no |
| <a name="input_allowed_security_groups"></a> [allowed\_security\_groups](#input\_allowed\_security\_groups) | List of existing Security Groups to be allowed to connect to the DocumentDB cluster |  <pre lang="json">list(string)</pre> |  <pre lang="json">[]</pre> |  no |
| <a name="input_apply_immediately"></a> [apply\_immediately](#input\_apply\_immediately) | Specifies whether any cluster modifications are applied immediately, or during the next maintenance window |  <pre lang="json">bool</pre> |  <pre lang="json">true</pre> |  no |
| <a name="input_context"></a> [context](#input\_context) | Single object for setting entire context at once.<br>See description of individual variables for details.<br>Leave string and numeric variables as `null` to use default value.<br>Individual variable settings (non-null) override settings in context object,<br>except for attributes and tags, which are merged. |  <pre lang="json">any</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_db_port"></a> [db\_port](#input\_db\_port) | The port the mongo database will listen on |  <pre lang="json">number</pre> |  <pre lang="json">27017</pre> |  no |
| <a name="input_default_database"></a> [default\_database](#input\_default\_database) | The name of the default database to create |  <pre lang="json">string</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_deletion_protection"></a> [deletion\_protection](#input\_deletion\_protection) | A value that indicates whether the DB cluster has deletion protection enabled |  <pre lang="json">bool</pre> |  <pre lang="json">false</pre> |  no |
| <a name="input_egress_from_port"></a> [egress\_from\_port](#input\_egress\_from\_port) | [from\_port]DocumentDB initial port range for egress (e.g. `0`) |  <pre lang="json">number</pre> |  <pre lang="json">0</pre> |  no |
| <a name="input_egress_protocol"></a> [egress\_protocol](#input\_egress\_protocol) | DocumentDB protocol for egress (e.g. `-1`, `tcp`) |  <pre lang="json">string</pre> |  <pre lang="json">"-1"</pre> |  no |
| <a name="input_egress_to_port"></a> [egress\_to\_port](#input\_egress\_to\_port) | [to\_port]DocumentDB initial port range for egress (e.g. `65535`) |  <pre lang="json">number</pre> |  <pre lang="json">0</pre> |  no |
| <a name="input_enable_performance_insights"></a> [enable\_performance\_insights](#input\_enable\_performance\_insights) | Enable performance insights |  <pre lang="json">bool</pre> |  <pre lang="json">false</pre> |  no |
| <a name="input_enabled_cloudwatch_logs_exports"></a> [enabled\_cloudwatch\_logs\_exports](#input\_enabled\_cloudwatch\_logs\_exports) | List of log types to export to cloudwatch. The following log types are supported: `audit`, `profiler` |  <pre lang="json">list(string)</pre> |  <pre lang="json">[]</pre> |  no |
| <a name="input_engine"></a> [engine](#input\_engine) | The name of the database engine to be used for this DB cluster. Defaults to `docdb`. Valid values: `docdb` |  <pre lang="json">string</pre> |  <pre lang="json">null</pre> |  no |
| <a name="input_engine_version"></a> [engine\_version](#input\_engine\_version) | The version number of the database engine to use |  <pre lang="json">string</pre> |  <pre lang="json">null</pre> |  no |
| <a name="input_master_password"></a> [master\_password](#input\_master\_password) | The password for the master DB user |  <pre lang="json">string</pre> |  <pre lang="json">""</pre> |  no |
| <a name="input_master_username"></a> [master\_username](#input\_master\_username) | The username for the master DB user |  <pre lang="json">string</pre> |  <pre lang="json">"db\_admin"</pre> |  no |
| <a name="input_preferred_backup_window"></a> [preferred\_backup\_window](#input\_preferred\_backup\_window) | Daily time range during which the backups happen |  <pre lang="json">string</pre> |  <pre lang="json">null</pre> |  no |
| <a name="input_preferred_maintenance_window"></a> [preferred\_maintenance\_window](#input\_preferred\_maintenance\_window) | The window to perform maintenance in. Syntax: `ddd:hh24:mi-ddd:hh24:mi`. |  <pre lang="json">string</pre> |  <pre lang="json">null</pre> |  no |
| <a name="input_primary_instance_class"></a> [primary\_instance\_class](#input\_primary\_instance\_class) | The instance class of the primary instances |  <pre lang="json">string</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_primary_instance_count"></a> [primary\_instance\_count](#input\_primary\_instance\_count) | The number of primary instances to create |  <pre lang="json">number</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_replica_instance_class"></a> [replica\_instance\_class](#input\_replica\_instance\_class) | The instance class of the replica instances |  <pre lang="json">string</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_replica_instance_count"></a> [replica\_instance\_count](#input\_replica\_instance\_count) | The number of replica instances to create |  <pre lang="json">number</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_retention_period"></a> [retention\_period](#input\_retention\_period) | Number of days to retain backups for |  <pre lang="json">number</pre> |  <pre lang="json">null</pre> |  no |
| <a name="input_skip_final_snapshot"></a> [skip\_final\_snapshot](#input\_skip\_final\_snapshot) | Determines whether a final DB snapshot is created before the DB cluster is deleted |  <pre lang="json">bool</pre> |  <pre lang="json">true</pre> |  no |
| <a name="input_subnet_ids"></a> [subnet\_ids](#input\_subnet\_ids) | The IDs of the subnets to deploy to |  <pre lang="json">list(string)</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_vpc_id"></a> [vpc\_id](#input\_vpc\_id) | The ID of the VPC to deploy to |  <pre lang="json">string</pre> |  <pre lang="json">n/a</pre> |  yes |
## Outputs

| Name | Description |
|------|-------------|
| <a name="output_cluster_id"></a> [cluster\_id](#output\_cluster\_id) | The cluster identifier |
| <a name="output_connection_url"></a> [connection\_url](#output\_connection\_url) | The connection url |
| <a name="output_endpoint"></a> [endpoint](#output\_endpoint) | The connection endpoint |
| <a name="output_password"></a> [password](#output\_password) | The master password |
| <a name="output_port"></a> [port](#output\_port) | The connection port |
| <a name="output_username"></a> [username](#output\_username) | The master username |


<!-- END_TF_DOCS -->
