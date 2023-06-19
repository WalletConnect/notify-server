local grafana         = import '../../grafonnet-lib/grafana.libsonnet';
local defaults        = import '../../grafonnet-lib/defaults.libsonnet';
local units           = import '../utils/units.libsonnet';

local panels          = grafana.panels;
local targets         = grafana.targets;
{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Available Memory',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.configuration.docdb_available_memory())
    .addPanelThreshold(
      op = 'lt',
      value = defaults.values.available_memory.threshold,
    )

    .setAlert(defaults.alerts.available_memory(
      namespace     = vars.namespace,
      title         = 'DocumentDB',
      env           = vars.environment,
      notifications = vars.notifications,
    ))

    .addTarget(targets.cloudwatch(
      refId       = 'Mem_Min',
      alias       = 'Freeable Memory (Min)',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      metricName  = 'FreeableMemory',
      statistic   = 'Minimum',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
    ))
    .addTarget(targets.cloudwatch(
      refId       = 'Mem_Avg',
      alias       = 'Freeable Memory (Avg)',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      metricName  = 'FreeableMemory',
      statistic   = 'Average',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
    ))
}
