local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Memory Utilization',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.overrides.memory(defaults.configuration.timeseries_resource))

    .setAlert(defaults.alerts.memory(
      namespace     = vars.namespace,
      title         = 'ECS',
      env           = vars.environment,
      notifications = vars.notifications,
    ))

    .addTarget(targets.cloudwatch(
      alias       = "Memory (${PROP('Stat')})",
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ECS',
      metricName  = 'MemoryUtilization',
      dimensions  = {
        ServiceName: vars.ecs_service_name
      },
      statistic   = 'Maximum',
      refId       = 'Mem_Max',
    ))
    .addTarget(targets.cloudwatch(
      alias       = "Memory (${PROP('Stat')})",
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ECS',
      metricName  = 'MemoryUtilization',
      dimensions  = {
        ServiceName: vars.ecs_service_name
      },
      statistic   = 'Average',
      refId       = 'Mem_Avg',
    ))
}
