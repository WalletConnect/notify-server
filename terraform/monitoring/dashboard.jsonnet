local grafana     = import 'grafonnet-lib/grafana.libsonnet';
local panels      = import 'panels/panels.libsonnet';

local dashboard   = grafana.dashboard;
local row         = grafana.row;
local annotation  = grafana.annotation;
local layout      = grafana.layout;

local ds    = {
  prometheus: {
    type: 'prometheus',
    uid:  std.extVar('prometheus_uid'),
  },
  cloudwatch: {
    type: 'cloudwatch',
    uid:  std.extVar('cloudwatch_uid'),
  },
};
local vars  = {
  namespace:        'Notify',
  environment:      std.extVar('environment'),
  notifications:    std.parseJson(std.extVar('notifications')),

  ecs_service_name: std.extVar('ecs_service_name'),
  ecs_cluster_name: std.extVar('ecs_cluster_name'),
  load_balancer:    std.extVar('load_balancer'),
  target_group:     std.extVar('target_group'),
};

////////////////////////////////////////////////////////////////////////////////

local height    = 8;
local pos       = grafana.layout.pos(height);

////////////////////////////////////////////////////////////////////////////////

dashboard.new(
  title         = std.extVar('dashboard_title'),
  uid           = std.extVar('dashboard_uid'),
  editable      = true,
  graphTooltip  = dashboard.graphTooltips.sharedCrosshair,
  timezone      = dashboard.timezones.utc,
)
.addAnnotation(
  annotation.new(
    target = {
      limit:    100,
      matchAny: false,
      tags:     [],
      type:     'dashboard',
    },
  )
)

.addPanels(layout.generate_grid([
  //////////////////////////////////////////////////////////////////////////////
  row.new('Application'),
    panels.app.http_request_rate(ds, vars)          { gridPos: pos._4 },
    panels.app.http_request_latency(ds, vars)       { gridPos: pos._4 },

    panels.app.subscribed_topics(ds, vars)          { gridPos: pos._4 },
    panels.app.subscribe_latency(ds, vars)          { gridPos: pos._4 },

    panels.app.relay_incomming_message_rate(ds, vars)               {gridPos: pos._6 },
    panels.app.relay_incomming_message_latency(ds, vars)            {gridPos: pos._6 },
    panels.app.relay_incomming_message_server_errors(ds, vars)      {gridPos: pos._6 },

    panels.app.relay_outgoing_message_rate(ds, vars)                {gridPos: pos._6 },
    panels.app.relay_outgoing_message_latency(ds, vars)             {gridPos: pos._6 },
    panels.app.relay_outgoing_message_failures(ds, vars)            {gridPos: pos._6 },

  row.new('Deprecated metrics'),
    panels.app.notify_latency(ds, vars)             { gridPos: pos._4 },
    panels.app.dispatched_notifications(ds, vars)   { gridPos: pos._4 },
    panels.app.send_failed(ds, vars)                { gridPos: pos._4 },
    panels.app.account_not_found(ds, vars)          { gridPos: pos._4 },

  //////////////////////////////////////////////////////////////////////////////
  row.new('ECS'),
    panels.ecs.cpu(ds, vars)                      { gridPos: pos._2 },
    panels.ecs.memory(ds, vars)                   { gridPos: pos._2 },

  //////////////////////////////////////////////////////////////////////////////
  row.new('RDS'),
    panels.rds.cpu(ds, vars)                      { gridPos: pos._4 },
    panels.rds.freeable_memory(ds, vars)          { gridPos: pos._4 },
    panels.rds.volume_bytes_used(ds, vars)        { gridPos: pos._4 },
    panels.rds.database_connections(ds, vars)     { gridPos: pos._4 },

  //////////////////////////////////////////////////////////////////////////////
  row.new('Load Balancer'),
    panels.lb.active_connections(ds, vars)        { gridPos: pos._2 },
    panels.lb.requests(ds, vars)                  { gridPos: pos._2 },

    panels.lb.healthy_hosts(ds, vars)             { gridPos: pos._3 },
    panels.lb.error_4xx(ds, vars)                 { gridPos: pos._3 },
    panels.lb.error_5xx(ds, vars)                 { gridPos: pos._3 },
]))
