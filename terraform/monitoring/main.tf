locals {
  load_balancer = join("/", slice(split("/", var.load_balancer_arn), 1, 4))

  target_group = split(":", var.target_group_arn)[5]

  opsgenie_notification_channel = "NNOynGwVz"
  notifications = (
    var.environment == "prod" ?
    [{ uid = local.opsgenie_notification_channel }] :
    []
  )
}
