local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Network Throughput',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.cloudwatch(
      alias       = 'Network Throughput',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
      metricName  = 'NetworkThroughput',
      statistic   = 'Maximum',
      refId       = 'NetworkThroughput',
    ))
}
