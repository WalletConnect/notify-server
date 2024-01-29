# `rds` module

This module creates a Postgres RDS cluster with IAM authentication.

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
| <a name="module_db_cluster"></a> [db\_cluster](#module\_db\_cluster) | terraform-aws-modules/rds-aurora/aws | 8.3.1 |
| <a name="module_this"></a> [this](#module\_this) | app.terraform.io/wallet-connect/label/null | 0.3.2 |

## Inputs
| Name | Description | Type | Default | Required |
|------|-------------|------|---------|:--------:|
| <a name="input_cloudwatch_logs_key_arn"></a> [cloudwatch\_logs\_key\_arn](#input\_cloudwatch\_logs\_key\_arn) | The ARN of the KMS key to use for encrypting CloudWatch logs |  <pre lang="json">string</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_cloudwatch_retention_in_days"></a> [cloudwatch\_retention\_in\_days](#input\_cloudwatch\_retention\_in\_days) | The number of days to retain CloudWatch logs for the DB instance |  <pre lang="json">number</pre> |  <pre lang="json">14</pre> |  no |
| <a name="input_context"></a> [context](#input\_context) | Single object for setting entire context at once.<br>See description of individual variables for details.<br>Leave string and numeric variables as `null` to use default value.<br>Individual variable settings (non-null) override settings in context object,<br>except for attributes and tags, which are merged. |  <pre lang="json">any</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_db_master_password"></a> [db\_master\_password](#input\_db\_master\_password) | The password for the master DB user |  <pre lang="json">string</pre> |  <pre lang="json">""</pre> |  no |
| <a name="input_db_master_username"></a> [db\_master\_username](#input\_db\_master\_username) | The username for the master DB user |  <pre lang="json">string</pre> |  <pre lang="json">"pgadmin"</pre> |  no |
| <a name="input_db_name"></a> [db\_name](#input\_db\_name) | The name of the default database in the cluster |  <pre lang="json">string</pre> |  <pre lang="json">"postgres"</pre> |  no |
| <a name="input_ingress_cidr_blocks"></a> [ingress\_cidr\_blocks](#input\_ingress\_cidr\_blocks) | The CIDR blocks to allow ingress from |  <pre lang="json">list(string)</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_instances"></a> [instances](#input\_instances) | The number of database instances to create |  <pre lang="json">number</pre> |  <pre lang="json">1</pre> |  no |
| <a name="input_max_capacity"></a> [max\_capacity](#input\_max\_capacity) | The maximum capacity for the Aurora cluster (in Aurora Capacity Units) |  <pre lang="json">number</pre> |  <pre lang="json">10</pre> |  no |
| <a name="input_min_capacity"></a> [min\_capacity](#input\_min\_capacity) | The minimum capacity for the Aurora cluster (in Aurora Capacity Units) |  <pre lang="json">number</pre> |  <pre lang="json">2</pre> |  no |
| <a name="input_subnet_ids"></a> [subnet\_ids](#input\_subnet\_ids) | The IDs of the subnets to deploy to |  <pre lang="json">list(string)</pre> |  <pre lang="json">n/a</pre> |  yes |
| <a name="input_vpc_id"></a> [vpc\_id](#input\_vpc\_id) | The VPC ID to create the security group in |  <pre lang="json">string</pre> |  <pre lang="json">n/a</pre> |  yes |
## Outputs

| Name | Description |
|------|-------------|
| <a name="output_database_name"></a> [database\_name](#output\_database\_name) | The name of the default database in the cluster |
| <a name="output_database_url"></a> [database\_url](#output\_database\_url) | The URL used to connect to the cluster |
| <a name="output_master_password_id"></a> [master\_password\_id](#output\_master\_password\_id) | The ID of the database master password in Secrets Manager |
| <a name="output_master_username"></a> [master\_username](#output\_master\_username) | The username for the master DB user |
| <a name="output_rds_cluster_arn"></a> [rds\_cluster\_arn](#output\_rds\_cluster\_arn) | The ARN of the cluster |
| <a name="output_rds_cluster_endpoint"></a> [rds\_cluster\_endpoint](#output\_rds\_cluster\_endpoint) | The cluster endpoint |
| <a name="output_rds_cluster_id"></a> [rds\_cluster\_id](#output\_rds\_cluster\_id) | The ID of the cluster |

<!-- END_TF_DOCS -->
