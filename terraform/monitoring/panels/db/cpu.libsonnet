local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'CPU Utilization',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.overrides.cpu(defaults.configuration.timeseries_resource))
    .setAlert(defaults.alerts.cpu(
      namespace     = 'Cast',
      title         = 'DocumentDB',
      env           = vars.environment,
      notifications = vars.notifications,
    ))

    .addTarget(targets.cloudwatch(
      alias       = 'CPU (Max)',
      datasource  = ds.cloudwatch,
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
      metricName  = 'CPUUtilization',
      namespace   = 'AWS/DocDB',
      statistic   = 'Maximum',
      refId       = 'CPU_Max',
    ))
    .addTarget(targets.cloudwatch(
      alias       = 'CPU (Avg)',
      datasource  = ds.cloudwatch,
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
      metricName  = 'CPUUtilization',
      namespace   = 'AWS/DocDB',
      statistic   = 'Average',
      refId       = 'CPU_Avg',
    ))
}
