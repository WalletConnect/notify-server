# Task execution role
data "aws_iam_role" "ecs_task_execution_role" {
  name = var.task_execution_role_name
}

resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_policy" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role_policy_attachment" "cloudwatch_write_policy" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/CloudWatchLogsFullAccess"
}

resource "aws_iam_role_policy_attachment" "prometheus_write_policy" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonPrometheusRemoteWriteAccess"
}

resource "aws_iam_role_policy_attachment" "ssm_read_only_policy" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMReadOnlyAccess"
}

resource "aws_iam_role_policy_attachment" "rds_auth_policy" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = var.rds_auth_policy_arn
}

resource "aws_iam_policy" "otel" {
  name   = "${module.this.id}-otel"
  path   = "/"
  policy = data.aws_iam_policy_document.otel.json
}
#tfsec:ignore:aws-iam-no-policy-wildcards
data "aws_iam_policy_document" "otel" {
  statement {
    actions = [
      "logs:PutLogEvents",
      "logs:CreateLogGroup",
      "logs:CreateLogStream",
      "logs:DescribeLogStreams",
      "logs:DescribeLogGroups",
      "ssm:GetParameters",
    ]
    resources = [
      "*"
    ]
  }
}
resource "aws_iam_role_policy_attachment" "ecs_task_execution_fetch_ghcr_secret_policy" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = aws_iam_policy.otel.arn
}

# GeoIP Bucket Access
resource "aws_iam_policy" "geoip_bucket_access" {
  name        = "${module.this.id}-geoip-bucket_access"
  path        = "/"
  description = "Allows ${module.this.id} to read from ${var.geoip_db_bucket_name}"

  policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "ListObjectsInGeoipBucket",
        "Effect" : "Allow",
        "Action" : ["s3:ListBucket"],
        "Resource" : ["arn:aws:s3:::${var.geoip_db_bucket_name}"]
      },
      {
        "Sid" : "AllObjectActionsInGeoipBucket",
        "Effect" : "Allow",
        "Action" : ["s3:CopyObject", "s3:GetObject", "s3:HeadObject"],
        "Resource" : ["arn:aws:s3:::${var.geoip_db_bucket_name}/*"]
      },
    ]
  })
}
resource "aws_iam_role_policy_attachment" "geoip_bucket_access" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = aws_iam_policy.geoip_bucket_access.arn
}

# Analytics Bucket Access
resource "aws_iam_policy" "datalake_bucket_access" {
  name        = "${module.this.id}-analytics-bucket_access"
  path        = "/"
  description = "Allows ${module.this.id} to read/write from ${var.analytics_datalake_bucket_name}"

  policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "ListObjectsInAnalyticsBucket",
        "Effect" : "Allow",
        "Action" : ["s3:ListBucket"],
        "Resource" : ["arn:aws:s3:::${var.analytics_datalake_bucket_name}"]
      },
      {
        "Sid" : "AllObjectActionsInAnalyticsBucket",
        "Effect" : "Allow",
        "Action" : "s3:*Object",
        "Resource" : ["arn:aws:s3:::${var.analytics_datalake_bucket_name}/notify/*"]
      },
      {
        "Sid" : "AllGenerateDataKeyForAnalyticsBucket",
        "Effect" : "Allow",
        "Action" : ["kms:GenerateDataKey"],
        "Resource" : [var.analytics_datalake_kms_key_arn]
      }
    ]
  })
}
resource "aws_iam_role_policy_attachment" "datalake_bucket_access" {
  role       = data.aws_iam_role.ecs_task_execution_role.name
  policy_arn = aws_iam_policy.datalake_bucket_access.arn
}
