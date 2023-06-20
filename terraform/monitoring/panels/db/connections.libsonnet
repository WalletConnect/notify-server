local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Database Connections',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.cloudwatch(
      alias       = 'Database Connections',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      metricName  = 'DatabaseConnections',
      statistic   = 'Average',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
    ))
}
