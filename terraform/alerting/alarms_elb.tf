resource "aws_cloudwatch_metric_alarm" "elb_5xx" {
  alarm_name        = "${local.alarm_prefix} - 5XX Threshold Breached"
  alarm_description = "${local.alarm_prefix} - The number of 5XX errors was over ${var.elb_5xx_threshold} for the period"

  namespace = module.cloudwatch.namespaces.ApplicationELB
  dimensions = {
    LoadBalancer : var.elb_load_balancer_arn
  }
  metric_name = module.cloudwatch.metrics.ApplicationELB.HTTPCode_ELB_5XX_Count

  evaluation_periods = local.evaluation_periods
  period             = local.period

  statistic           = module.cloudwatch.statistics.Sum
  comparison_operator = module.cloudwatch.operators.GreaterThanOrEqualToThreshold
  threshold           = var.elb_5xx_threshold
  treat_missing_data  = "breaching"

  alarm_actions             = [aws_sns_topic.cloudwatch_webhook.arn]
  insufficient_data_actions = [aws_sns_topic.cloudwatch_webhook.arn]
}
