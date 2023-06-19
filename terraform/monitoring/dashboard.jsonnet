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
  namespace:        'Cast',
  environment:      std.extVar('environment'),
  notifications:    std.parseJson(std.extVar('notifications')),

  ecs_service_name: std.extVar('ecs_service_name'),
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
  row.new('Database'),
    panels.db.available_memory(ds, vars)          { gridPos: pos._2 },
    panels.db.cpu(ds, vars)                       { gridPos: pos._2 },
    panels.db.net_throughput(ds, vars)            { gridPos: pos._2 },
    panels.db.write_latency(ds, vars)             { gridPos: pos._2 },

  row.new('Application'),
    panels.app.client_registrations(ds, vars)     { gridPos: pos._2 },
    panels.app.dispatched_notifications(ds, vars) { gridPos: pos._2 },
    panels.app.send_failed(ds, vars)              { gridPos: pos._2 },
    panels.app.account_not_found(ds, vars)        { gridPos: pos._2 },

  row.new('Load Balancer'),
    panels.lb.requests(ds, vars)                  { gridPos: pos._1 },
    panels.lb.error_4xx(ds, vars)                 { gridPos: pos._2 },
    panels.lb.error_5xx(ds, vars)                 { gridPos: pos._2 },
]))
