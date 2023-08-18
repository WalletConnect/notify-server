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
  docdb_cluster_id: std.extVar('docdb_cluster_id'),
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
    panels.app.subscribed_project_topics(ds, vars)  { gridPos: pos._2 },
    panels.app.subscribed_client_topics(ds, vars)   { gridPos: pos._2 },
    panels.app.dispatched_notifications(ds, vars)   { gridPos: pos._2 },
    panels.app.send_failed(ds, vars)                { gridPos: pos._2 },
    panels.app.account_not_found(ds, vars)          { gridPos: pos._2 },
    // TODO send latency (avg & max)
    // TODO subscribe latency (avg & max)

  //////////////////////////////////////////////////////////////////////////////
  row.new('ECS'),
    panels.ecs.cpu(ds, vars)                      { gridPos: pos._2 },
    panels.ecs.memory(ds, vars)                   { gridPos: pos._2 },

  //////////////////////////////////////////////////////////////////////////////
  row.new('DocumentDB'),
    panels.db.available_memory(ds, vars)          { gridPos: pos._3 },
    panels.db.cpu(ds, vars)                       { gridPos: pos._3 },
    panels.db.connections(ds, vars)               { gridPos: pos._3 },

    panels.db.low_mem_op_throttled(ds, vars)      { gridPos: pos._3 },
    panels.db.volume(ds, vars)                    { gridPos: pos._3 },
    panels.db.buffer_cache_hit_ratio(ds, vars)    { gridPos: pos._3 },

    panels.db.net_throughput(ds, vars)            { gridPos: pos._2 },
    panels.db.write_latency(ds, vars)             { gridPos: pos._2 },

  //////////////////////////////////////////////////////////////////////////////
  row.new('Load Balancer'),
    panels.lb.active_connections(ds, vars)        { gridPos: pos._2 },
    panels.lb.requests(ds, vars)                  { gridPos: pos._2 },

    panels.lb.healthy_hosts(ds, vars)             { gridPos: pos._3 },
    panels.lb.error_4xx(ds, vars)                 { gridPos: pos._3 },
    panels.lb.error_5xx(ds, vars)                 { gridPos: pos._3 },
]))
