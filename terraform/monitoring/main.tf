locals {
  load_balancer = join("/", slice(split("/", var.load_balancer_arn), 1, 4))

  opsgenie_notification_channel = "NNOynGwVz"
  notifications = (
    var.environment == "prod" ?
    [{ uid = local.opsgenie_notification_channel }] :
    []
  )
}
