local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Connections',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'DatabaseConnections',
      dimensions  = {
        DBClusterIdentifier: vars.rds_cluster_id,
      },
      matchExact  = true,
      statistic     = 'Maximum',
    ))
}
