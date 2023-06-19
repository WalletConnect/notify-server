local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local threshold = 100;

local _configuration = defaults.configuration.timeseries
  .withSoftLimit(
    axisSoftMin = 0,
    axisSoftMax = threshold * 2,
  )
  .withThresholdStyle(grafana.fieldConfig.thresholdStyle.dashed)
  .addThreshold({
    color : defaults.values.colors.critical,
    value : threshold,
  })
  .withColor(grafana.fieldConfig.colorMode.thresholds);


local _alert(namespace, env, notifications) = grafana.alert.new(
  namespace     = namespace,
  name          = "%(env)s - Failing on send (communicating with relay)"                                    % { env: env },
  message       = '%(env)s - Failing to send messages, potential problem with Cast <-> Relay communication' % { env: env },
  notifications = notifications,
  conditions    = [
    grafana.alertCondition.new(
      evaluatorParams = [ threshold ],
      evaluatorType   = 'gt',
      operatorType    = 'and',
      queryRefId      = 'Failed',
      queryTimeStart  = '5m',
      queryTimeEnd    = 'now',
      reducerType     = grafana.alert_reducers.Sum
    ),
  ],
);

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Failed to Send',
      datasource  = ds.prometheus,
    )
    .configure(_configuration)

    .setAlert(_alert(vars.namespace, vars.environment, vars.notifications))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by(aws_ecs_task_revision) (increase(dispatched_notifications{type="failed"}[5m]))',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId       = 'Failed',
    ))
}
