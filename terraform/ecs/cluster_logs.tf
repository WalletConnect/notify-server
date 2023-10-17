data "aws_caller_identity" "this" {}

#-------------------------------------------------------------------------------
# KMS Key

resource "aws_kms_key" "cloudwatch-logs" {
  description         = "KMS key for encrypting CloudWatch Logs"
  enable_key_rotation = true
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "Enable IAM User Permissions"
        Effect = "Allow"
        Principal = {
          AWS = data.aws_caller_identity.this.account_id
        }
        Action   = "kms:*"
        Resource = "*"
      },
      {
        Sid    = "AllowCloudWatchLogs"
        Effect = "Allow"
        Principal = {
          Service = "logs.${module.this.region}.amazonaws.com"
        }
        Action = [
          "kms:Encrypt*",
          "kms:Decrypt*",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
          "kms:Describe*"
        ]
        Resource = "*"
      },
    ]
  })
}

resource "aws_kms_alias" "cloudwatch-logs" {
  name          = "alias/${module.this.id}-cloudwatch-logs"
  target_key_id = aws_kms_key.cloudwatch-logs.key_id
}

#-------------------------------------------------------------------------------
# Log groups

resource "aws_cloudwatch_log_group" "cluster" {
  name              = "${module.this.id}-app-logs"
  kms_key_id        = aws_kms_key.cloudwatch-logs.arn
  retention_in_days = 14
}

resource "aws_cloudwatch_log_group" "otel" {
  name              = "${module.this.id}-aws-otel-sidecar-collector"
  kms_key_id        = aws_kms_key.cloudwatch-logs.arn
  retention_in_days = 14
}

resource "aws_cloudwatch_log_group" "prometheus_proxy" {
  name              = "${module.this.id}-sigv4-prometheus-proxy"
  kms_key_id        = aws_kms_key.cloudwatch-logs.arn
  retention_in_days = 14
}
