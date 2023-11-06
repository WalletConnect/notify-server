local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Subscribed Topics',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'subscribed_project_topics',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'SubscribedProjectTopics',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'subscribed_subscriber_topics',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'SubscribedSubscriberTopics',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'subscribed_project_topics + subscribed_subscriber_topics',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'SubscribedTopics',
    ))
}
