resource "aws_cloudwatch_metric_alarm" "docdb_cpu_utilization" {
  alarm_name        = "${local.alarm_prefix} - DocumentDB CPU Utilization"
  alarm_description = "${local.alarm_prefix} - DocumentDB CPU utilization is high (over ${var.docdb_cpu_threshold}%)"

  namespace = module.cloudwatch.namespaces.DocumentDB
  dimensions = {
    DBClusterIdentifier = var.docdb_cluster_id
  }
  metric_name = module.cloudwatch.metrics.DocumentDB.CPUUtilization

  evaluation_periods = local.evaluation_periods
  period             = local.period

  statistic           = module.cloudwatch.statistics.Average
  comparison_operator = module.cloudwatch.operators.GreaterThanOrEqualToThreshold
  threshold           = var.docdb_cpu_threshold
  treat_missing_data  = "breaching"

  alarm_actions             = [aws_sns_topic.cloudwatch_webhook.arn]
  insufficient_data_actions = [aws_sns_topic.cloudwatch_webhook.arn]
}

resource "aws_cloudwatch_metric_alarm" "docdb_available_memory" {
  alarm_name        = "${local.alarm_prefix} - DocumentDB Available Memory"
  alarm_description = "${local.alarm_prefix} - DocumentDB available memory is low (less than ${var.docdb_memory_threshold}GiB)"

  namespace = module.cloudwatch.namespaces.DocumentDB
  dimensions = {
    DBClusterIdentifier = var.docdb_cluster_id
  }
  metric_name = module.cloudwatch.metrics.DocumentDB.FreeableMemory

  evaluation_periods = local.evaluation_periods
  period             = local.period

  statistic           = module.cloudwatch.statistics.Average
  comparison_operator = module.cloudwatch.operators.LessThanOrEqualToThreshold
  threshold           = var.docdb_memory_threshold * pow(1000, 3)
  treat_missing_data  = "breaching"

  alarm_actions             = [aws_sns_topic.cloudwatch_webhook.arn]
  insufficient_data_actions = [aws_sns_topic.cloudwatch_webhook.arn]
}

resource "aws_cloudwatch_metric_alarm" "docdb_low_memory_throttling" {
  alarm_name        = "${local.alarm_prefix} - DocumentDB Low Memory Throttling"
  alarm_description = "${local.alarm_prefix} - DocumentDB is throttling operations due to low memory"

  namespace = module.cloudwatch.namespaces.DocumentDB
  dimensions = {
    DBClusterIdentifier = var.docdb_cluster_id
  }
  metric_name = module.cloudwatch.metrics.DocumentDB.LowMemNumOperationsThrottled

  evaluation_periods = local.evaluation_periods
  period             = local.period

  statistic           = module.cloudwatch.statistics.Maximum
  comparison_operator = module.cloudwatch.operators.GreaterThanThreshold
  threshold           = var.docdb_low_memory_throttling_threshold
  treat_missing_data  = "breaching"

  alarm_actions             = [aws_sns_topic.cloudwatch_webhook.arn]
  insufficient_data_actions = [aws_sns_topic.cloudwatch_webhook.arn]
}
